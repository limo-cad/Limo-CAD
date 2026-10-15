//! Desktop events reach the owned application's OS window and real Winit loop.
//! MCP only opens/focuses controls, observes results, and captures that window.
use crate::{
    native_fixture::{capture, control, controls, ui},
    replay::Client,
};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut server = None;
    let mut out = None;
    let mut desktop_input = false;
    let mut ime_libpinyin = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => {
                server = Some(PathBuf::from(args.next().context("Missing --server path")?))
            }
            "--out" => out = Some(PathBuf::from(args.next().context("Missing --out path")?)),
            "--desktop-input" => desktop_input = true,
            "--ime-libpinyin" => ime_libpinyin = true,
            _ => bail!("Unknown native-platform option {arg}"),
        }
    }
    ensure!(desktop_input, "Use --desktop-input on a disposable desktop: this check focuses its own window and uses the system text clipboard");
    ensure!(
        !ime_libpinyin
            || cfg!(target_os = "linux")
                && std::env::var("NBCAD_NATIVE_IME_TEST").as_deref() == Ok("1"),
        "Run --ime-libpinyin only through the isolated Linux IME runner"
    );
    let server = server
        .context("Use --server for the dev-bevy-host binary")?
        .canonicalize()?;
    let out = out.context("Use --out for an empty evidence directory")?;
    ensure!(out.is_absolute(), "Evidence directory must be absolute");
    ensure!(
        !out.exists() || fs::read_dir(&out)?.next().is_none(),
        "Preserve existing evidence; choose an empty directory"
    );
    fs::create_dir_all(&out)?;
    let result = exercise(&server, &out, ime_libpinyin);
    let report = match &result {
        Ok(evidence) => evidence.clone(),
        Err(error) => {
            json!({"status":"failed", "error":format!("{error:#}"), "platform":std::env::consts::OS})
        }
    };
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    result.map(|_| ())
}

fn exercise(server: &Path, out: &Path, ime_libpinyin: bool) -> Result<Value> {
    // A fresh registry prevents selecting or modifying any pre-existing design.
    let sessions = out.join("sessions");
    fs::create_dir(&sessions)?;
    let mut command = Command::new(server);
    command
        .current_dir(&sessions)
        .env("NBCAD_SESSION_DIR", &sessions);
    let mut client = Client::start_command(command, Some(Duration::from_secs(45)))?;
    let session = wait_for_owned_window(&mut client, &sessions)?;
    // Registry/model publication precedes the first laid-out interface frame.
    // Pin only the session proved to belong to this child, then await that frame.
    client.call("cad_attach", json!({"session_id":session}))?;
    wait_for_interface(&mut client, &session)?;
    let document = client.call("cad_document", json!({}))?;
    ensure!(
        document["features"].as_array().is_some_and(Vec::is_empty),
        "Owned document is not blank"
    );
    let driver = Driver::new(client.process_id(), out)?;
    capture(&mut client, out, "startup")?;
    driver.event("focus")?;
    control(&mut client, "File", None)?;
    control(&mut client, "Rename Project…", None)?;
    control(&mut client, "Project name", None)?;
    let initial = text_state(&mut client)?;
    let name = initial["value"]
        .as_str()
        .context("Rename field has no value")?
        .to_owned();
    ensure!(!name.is_empty(), "Initial document name is empty");
    capture(&mut client, out, "focused")?;

    // Preserve text clipboard in memory; never serialize the previous contents.
    // These are disposable desktop checks: non-text clipboard formats are not preserved.
    let previous_clipboard = driver.clipboard_read()?;
    let checked = (|| -> Result<Value> {
        driver.event("select-all")?;
        let selected = wait_field(&mut client, |field| {
            field["value"] == name && selected_all(field, &name)
        })
        .context("Select the original name with the OS shortcut")?;
        capture(&mut client, out, "selected")?;
        // A distinct value makes the subsequent read an acknowledgement that
        // the application processed Copy, not a match against old clipboard data.
        driver.clipboard_write("nbcad-copy-pending")?;
        driver.event("copy")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if driver.clipboard_read()? == name {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "OS copy did not put the selected name on the clipboard"
            );
            thread::sleep(Duration::from_millis(50));
        }
        driver.event("right")?;
        let end = name.encode_utf16().count();
        let collapsed = wait_field(&mut client, |field| {
            field["value"] == name && field["selection"] == json!({"start":end,"end":end})
        })
        .context("Collapse the original selection with Right")?;
        capture(&mut client, out, "caret")?;
        // Unicode clipboard input is deliberately not reported as IME composition.
        let unicode = "Café 零件 Ω 🦀";
        driver.clipboard_write(unicode)?;
        driver.event("select-all")?;
        driver.event("paste")?;
        let pasted = wait_field(&mut client, |field| field["value"] == unicode)
            .context("Paste Unicode text from the OS clipboard")?;
        capture(&mut client, out, "unicode")?;
        driver.event("select-all")?;
        let unicode_selected = wait_field(&mut client, |field| {
            field["value"] == unicode && selected_all(field, unicode)
        })
        .context("Select the Unicode text with the OS shortcut")?;
        fs::write(
            out.join("unicode-selected.json"),
            serde_json::to_vec_pretty(&unicode_selected)?,
        )?;
        // The clipboard still contains `unicode` from Paste. Replace it before
        // Copy so a delayed Ctrl+C cannot race the later restoration paste.
        driver.clipboard_write("nbcad-copy-pending")?;
        driver.event("copy")?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if driver.clipboard_read()? == unicode {
                break;
            }
            ensure!(
                Instant::now() < deadline,
                "Unicode copy did not preserve the selected text"
            );
            thread::sleep(Duration::from_millis(50));
        }
        driver.clipboard_write(&name)?;
        let before_restore = text_state(&mut client)?;
        fs::write(
            out.join("before-restore.json"),
            serde_json::to_vec_pretty(&before_restore)?,
        )?;
        ensure!(
            before_restore["value"] == unicode && selected_all(&before_restore, unicode),
            "Copying the Unicode selection changed the field: {before_restore}"
        );
        driver.event("paste")?;
        let restored = wait_field(&mut client, |field| field["value"] == name)
            .context("Restore the original name by pasting from the OS clipboard")?;
        let ime = if ime_libpinyin {
            Some(exercise_ime(&mut client, &driver, out, &name)?)
        } else {
            None
        };
        let snapshot = ui(&mut client, json!({"action":"inspect"}))?;
        fs::write(
            out.join("final-inspect.json"),
            serde_json::to_vec_pretty(&snapshot)?,
        )?;
        // This assertion catches field modifier/navigation events being mistaken
        // for SetValue; selection alone can succeed while an error is displayed.
        ensure!(
            !snapshot.to_string().contains("Project name requires text"),
            "Keyboard navigation emitted a spurious field error"
        );
        capture(&mut client, out, "restored")?;
        let png = fs::read(out.join("selected.png"))?;
        ensure!(
            png.len() >= 24 && &png[..8] == b"\x89PNG\r\n\x1a\n",
            "Native capture is not a PNG"
        );
        Ok(json!({
            "status":"passed", "platform":std::env::consts::OS,
            "event_source":driver.source(), "owned_pid":client.process_id(), "session":session,
            "initial":initial, "selected":selected, "collapsed":collapsed,
            "unicode":pasted, "restored":restored,
            "ime":ime,
            "capture_pixels":[u32::from_be_bytes(png[16..20].try_into().unwrap()),u32::from_be_bytes(png[20..24].try_into().unwrap())],
            "x11_scale_factor":std::env::var("WINIT_X11_SCALE_FACTOR").ok(),
            "not_tested":[if ime_libpinyin { "Other IME engines/platforms" } else { "IME composition" }, "physical keyboard", "monitor DPI transition", "visual correctness without reviewing the captures"]
        }))
    })();
    let restored = driver.clipboard_write(&previous_clipboard);
    match checked {
        Ok(evidence) => {
            restored?;
            Ok(evidence)
        }
        Err(error) => {
            let _ = restored;
            Err(error)
        }
    }
}

fn exercise_ime(client: &mut Client, driver: &Driver, out: &Path, original: &str) -> Result<Value> {
    driver.event("select-all")?;
    driver.event("backspace")?;
    wait_field(client, |field| field["value"] == "")?;
    driver.event("ime-enable")?;
    driver.event("ime-preedit")?;
    // The committed buffer must exclude the provisional pinyin. The real
    // preedit/underline and OS candidate popup are retained as pixel evidence.
    thread::sleep(Duration::from_millis(200));
    let preedit = text_state(client)?;
    ensure!(
        preedit["value"] == "",
        "IME keystrokes leaked into committed text: {preedit}"
    );
    capture(client, out, "ime-preedit")?;
    let state = ui(client, json!({"action":"inspect"}))?;
    let request = json!({"field":preedit["bounds"], "client":state["ui"]["client"], "capture":out.join("ime-popup.png")});
    let popup: Value =
        serde_json::from_str(&driver.invoke("ime-evidence", Some(&request.to_string()))?)?;
    fs::write(
        out.join("ime-popup.json"),
        serde_json::to_vec_pretty(&popup)?,
    )?;
    driver.event("ime-commit")?;
    let committed = wait_field(client, |field| field["value"] == "你好")?;
    capture(client, out, "ime-committed")?;
    driver.event("ime-preedit")?;
    thread::sleep(Duration::from_millis(200));
    ensure!(
        text_state(client)?["value"] == "你好",
        "A second composition changed committed text before acceptance"
    );
    driver.event("ime-cancel")?;
    driver.event("home")?;
    // Home is ignored by our adapter during active composition. Observing its
    // effect after Escape proves composition ended, beyond unchanged text alone.
    let cancelled = wait_field(client, |field| {
        field["value"] == "你好" && field["selection"] == json!({"start":0,"end":0})
    })?;
    capture(client, out, "ime-cancelled")?;
    driver.event("ime-disable")?;
    driver.clipboard_write(original)?;
    driver.event("select-all")?;
    driver.event("paste")?;
    wait_field(client, |field| field["value"] == original)?;
    Ok(
        json!({"engine":"IBus libpinyin over XIM", "event_source":"X11 XTEST keystrokes", "preedit":preedit, "popup":popup, "committed":committed, "cancelled":cancelled}),
    )
}

fn selected_all(field: &Value, value: &str) -> bool {
    field["selection"] == json!({"start":0,"end":value.encode_utf16().count()})
}
fn text_state(client: &mut Client) -> Result<Value> {
    let inspected = ui(client, json!({"action":"inspect"}))?;
    let field = controls(&inspected)
        .find(|c| c["label"] == "Project name")
        .context("Rename field is not visible")?;
    ensure!(
        inspected["ui"]["focused_control"] == field["id"],
        "Rename field lost focus: {inspected}"
    );
    Ok(field.clone())
}
fn wait_field(client: &mut Client, expected: impl Fn(&Value) -> bool) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let field = text_state(client)?;
        if expected(&field) {
            return Ok(field);
        }
        ensure!(
            Instant::now() < deadline,
            "OS input did not produce expected text/selection: {field}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}
pub(super) fn wait_for_owned_window(client: &mut Client, sessions: &Path) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        ensure!(
            client.is_running()?,
            "Owned native host exited before publishing its window"
        );
        let mut found = BTreeSet::new();
        if let Ok(entries) = fs::read_dir(sessions.join("_ui/processes")) {
            for entry in entries.flatten() {
                let Ok(body) = fs::read(entry.path()) else {
                    continue;
                };
                let Ok(lease) = serde_json::from_slice::<Value>(&body) else {
                    continue;
                };
                if lease["pid"].as_u64() != Some(u64::from(client.process_id())) {
                    continue;
                }
                for window in lease["windows"].as_array().into_iter().flatten() {
                    if let Some(session) = window["active_session_id"].as_str() {
                        found.insert(session.to_owned());
                    }
                }
            }
        }
        ensure!(
            found.len() <= 1,
            "Owned native host published multiple sessions"
        );
        if let Some(session) = found.into_iter().next() {
            if sessions.join(&session).join("model.json").is_file() {
                return Ok(session);
            }
        }
        ensure!(
            Instant::now() < deadline,
            "Owned native window was not ready within 45 seconds"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

pub(super) fn wait_for_interface(client: &mut Client, session: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        ensure!(
            client.is_running()?,
            "Owned native host exited before interface readiness"
        );
        let result = client.call("cad_interface", json!({"action":"inspect"}));
        if let Ok(state) = &result {
            if state["status"] == "applied" {
                ensure!(
                    state["active_session_id"] == session,
                    "Interface belongs to an unexpected document: {state}"
                );
                return Ok(());
            }
        }
        ensure!(
            Instant::now() < deadline,
            "Owned interface did not become ready: {result:?}"
        );
        thread::sleep(Duration::from_millis(100));
    }
}

pub(super) struct Driver {
    pid: u32,
    helper: PathBuf,
}
impl Driver {
    pub(super) fn new(pid: u32, out: &Path) -> Result<Self> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("platform");
        #[cfg(target_os = "macos")]
        let helper = {
            let helper = out.join("native-input-macos");
            let status = Command::new("swiftc")
                .arg(root.join("native-input-macos.swift"))
                .arg("-o")
                .arg(&helper)
                .status()?;
            ensure!(
                status.success(),
                "Cannot compile the CoreGraphics input helper"
            );
            helper
        };
        #[cfg(target_os = "windows")]
        let helper = root.join("native-input-windows.ps1");
        #[cfg(target_os = "linux")]
        let helper = root.join("native-input-linux.sh");
        let _ = out;
        Ok(Self { pid, helper })
    }
    fn source(&self) -> &'static str {
        if cfg!(target_os = "macos") {
            "CoreGraphics OS keyboard events"
        } else if cfg!(target_os = "windows") {
            "Windows SendInput"
        } else {
            "X11 XTEST through xdotool"
        }
    }
    fn command(&self, operation: &str) -> Command {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut c = Command::new("powershell.exe");
            c.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&self.helper);
            c
        };
        #[cfg(target_os = "linux")]
        let mut command = {
            let mut c = Command::new("bash");
            c.arg(&self.helper);
            c
        };
        #[cfg(target_os = "macos")]
        let mut command = Command::new(&self.helper);
        command.arg(self.pid.to_string()).arg(operation);
        command
    }
    pub(super) fn invoke(&self, operation: &str, input: Option<&str>) -> Result<String> {
        let mut command = self.command(operation);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().context("Start OS input helper")?;
        if let Some(input) = input {
            child.stdin.take().unwrap().write_all(input.as_bytes())?;
        } else {
            drop(child.stdin.take());
        }
        let deadline = Instant::now() + Duration::from_secs(20);
        while child.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("OS input helper {operation} exceeded 20 seconds");
            }
            thread::sleep(Duration::from_millis(25));
        }
        let output = child.wait_with_output()?;
        ensure!(
            output.status.success(),
            "OS input helper {operation} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).context("OS helper output was not UTF-8")
    }
    pub(super) fn event(&self, operation: &str) -> Result<()> {
        self.invoke(operation, None).map(|_| ())
    }
    fn clipboard_read(&self) -> Result<String> {
        self.invoke("clipboard-read", None)
    }
    fn clipboard_write(&self, value: &str) -> Result<()> {
        self.invoke("clipboard-write", Some(value))?;
        // xclip 0.13 forks after buffering XSetSelectionOwner, before the
        // daemon services requests. Observe the OS value before sending paste;
        // process exit alone is not a clipboard-ownership acknowledgement.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if self.clipboard_read()? == value {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "OS clipboard write was not acknowledged"
            );
            thread::sleep(Duration::from_millis(25));
        }
    }
}
