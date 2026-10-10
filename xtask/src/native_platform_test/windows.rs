//! Windows fixtures share the production, owner-fenced MCP computer-control path.
use crate::replay::Client;
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    fs,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

pub(crate) struct Driver {
    pub(crate) pid: u32,
    pub(crate) helper: PathBuf,
    image: PathBuf,
    start_time: u64,
    sessions: PathBuf,
    process_instance: String,
    window_id: String,
    diagnostics: PathBuf,
    preparation_sequence: Cell<u32>,
    text_target: RefCell<Option<Value>>,
    client: RefCell<Client>,
}

struct Lease {
    process_instance: String,
    window_id: String,
    session: String,
    document: String,
}

impl Driver {
    pub(crate) fn new(pid: u32, out: &Path) -> Result<Self> {
        let (image, start_time) = process_identity(pid)?;
        if crate::deploy_native::managed_install_exists()? {
            ensure!(
                crate::deploy_native::verify_installed_runtime()?.canonicalize()? == image,
                "Windows input must use the current canonical installed CAD executable"
            );
        }
        let sessions = private_sessions(out, pid, &image)?;
        let lease = active_lease(&sessions, pid)?;
        let mut command = Client::worker_command(&image);
        command
            .current_dir(image.parent().context("CAD executable directory")?)
            .env("LIMO_CAD_SESSION_DIR", &sessions)
            .env("LIMO_CAD_DESKTOP_BIN", &image);
        let client = Client::start_command(command, Some(Duration::from_secs(45)))?;
        prepare_hosted_arm_desktop(out)?;
        Ok(Self {
            pid,
            helper: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("platform/native-windows-ime-session.ps1"),
            image,
            start_time,
            sessions,
            process_instance: lease.process_instance,
            window_id: lease.window_id,
            diagnostics: out.join("input-helper.jsonl"),
            preparation_sequence: Cell::new(0),
            text_target: RefCell::new(None),
            client: RefCell::new(client),
        })
    }

    pub(crate) fn source(&self) -> &'static str {
        "Rust cad_computer_control / Enigo OS input"
    }

    fn check_process(&self) -> Result<()> {
        let (image, start_time) = process_identity(self.pid)?;
        ensure!(
            image == self.image && start_time == self.start_time,
            "Owned CAD process was replaced; no further input was sent"
        );
        Ok(())
    }

    fn observe(&self) -> Result<Value> {
        // Interface readiness can precede the heartbeat's bootstrap-to-document
        // promotion. Wait for publication; never retry an OS-input operation.
        let deadline = Instant::now() + Duration::from_secs(45);
        let lease = loop {
            self.check_process()?;
            let lease = active_lease(&self.sessions, self.pid)?;
            ensure!(
                lease.process_instance == self.process_instance
                    && lease.window_id == self.window_id,
                "Owned CAD process or window instance changed"
            );
            let heartbeat = fs::read(self.sessions.join(&lease.session).join("heartbeat.json"))
                .ok()
                .and_then(|body| serde_json::from_slice::<Value>(&body).ok());
            let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
            if heartbeat.as_ref().is_some_and(|value| {
                value["interface_version"] == 1
                    && value["session_id"] == lease.session
                    && value["document_id"] == lease.document
                    && value["project_session_id"] == lease.document
                    && value["process_instance_id"] == self.process_instance
                    && value["window_id"] == self.window_id
                    && value["updated_ms"]
                        .as_u64()
                        .is_some_and(|updated| now.saturating_sub(updated) <= 30_000)
            }) {
                break lease;
            }
            ensure!(
                Instant::now() < deadline,
                "Owned desktop publication did not match the current lease within 45 seconds: session {} document {}; heartbeat {heartbeat:?}",
                lease.session,
                lease.document
            );
            thread::sleep(Duration::from_millis(100));
        };
        let observed = self.client.borrow_mut().call(
            "cad_computer_control",
            json!({"action":"observe","session_id":lease.session}),
        )?;
        ensure!(
            observed["status"] == "observed"
                && observed["owner"]["pid"] == self.pid
                && observed["owner"]["process_instance_id"] == self.process_instance
                && observed["owner"]["window_id"] == self.window_id
                && observed["owner"]["document_id"] == lease.document
                && observed["owner"]["session_id"] == lease.session,
            "Computer observation did not qualify the owned CAD window: {observed}"
        );
        self.check_process()?;
        Ok(observed)
    }

    fn observe_key_target(&self) -> Result<Value> {
        let observed = self.observe()?;
        if observed["foreground"] != true && hosted_arm_desktop() {
            let foreground = foreground_diagnostics();
            self.record(
                "prepare-before-key",
                &json!({"observed_owner":observed["owner"],"foreground_window":foreground}),
            )?;
            let sequence = self.preparation_sequence.get() + 1;
            self.preparation_sequence.set(sequence);
            let evidence = self
                .diagnostics
                .parent()
                .context("Native input evidence directory")?
                .join(format!("runner-input-desktop-{sequence}.json"));
            prepare_hosted_arm_desktop_at(&evidence, foreground["hwnd"].as_u64())?;
            let prepared = self.observe()?;
            super::keyboard::require_same_target(&observed, &prepared)?;
            return Ok(prepared);
        }
        Ok(observed)
    }

    fn send(&self, observed: &Value, mut request: Value) -> Result<Value> {
        self.check_process()?;
        request["session_id"] = observed["owner"]["session_id"].clone();
        request["observation"] = observed["observation"].clone();
        let action = request["action"].clone();
        let focus = request["action"] == "focus";
        if focus {
            self.record_focus_state("focus-before", observed, None);
        }
        let result = self
            .client
            .borrow_mut()
            .call("cad_computer_control", request);
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(error) => {
                if focus {
                    self.record_focus_state("focus-failed", observed, Some(&error));
                }
                if let Err(diagnostic_error) = self.record(
                    "computer-control",
                    &json!({"status":"failed","action":action,
                    "error":format!("{error:#}"),"observed_owner":observed["owner"],
                    "observed_foreground":observed["foreground"],
                    "observed_window_handle":observed["window_handle"],
                    "native_window_diagnostics":observed["native_window_diagnostics"]}),
                ) {
                    eprintln!("Could not retain input refusal diagnostics: {diagnostic_error:#}");
                }
                return Err(error);
            }
        };
        self.record("computer-control", &receipt)?;
        ensure!(
            matches!(receipt["status"].as_str(), Some("input_sent" | "focused")),
            "OS input did not complete; inspect its receipt and do not retry blindly: {receipt}"
        );
        Ok(receipt)
    }

    fn record_focus_state(&self, operation: &str, observed: &Value, error: Option<&anyhow::Error>) {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
        };

        // Observational only: never activate another window or retry input.
        let hwnd = unsafe { GetForegroundWindow() };
        let mut pid = 0;
        let mut title = [0_u16; 256];
        let (window_thread, title_length) = if hwnd.0.is_null() {
            (0, 0)
        } else {
            unsafe {
                (
                    GetWindowThreadProcessId(hwnd, Some(&mut pid)),
                    GetWindowTextW(hwnd, &mut title),
                )
            }
        };
        let receipt = json!({
            "owner":observed["owner"],
            "observed_foreground":observed["foreground"],
            "presented":observed["presented"],
            "minimized":observed["minimized"],
            "target_kind":observed["target_kind"],
            "window_handle":observed["window_handle"],
            "main_window_handle":observed["main_window_handle"],
            "harness_pid":std::process::id(),
            "worker_pid":self.client.borrow().process_id(),
            "foreground_window_handle":hwnd.0 as usize,
            "foreground_pid":pid,
            "foreground_thread":window_thread,
            "foreground_title":String::from_utf16_lossy(&title[..title_length.max(0) as usize]),
            "error":error.map(|error|format!("{error:#}")),
        });
        if let Err(error) = self.record(operation, &receipt) {
            // A diagnostics failure must not replace the original focus refusal.
            eprintln!("Could not retain read-only focus diagnostics: {error:#}");
        }
    }

    fn record(&self, operation: &str, receipt: &Value) -> Result<()> {
        let mut output = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.diagnostics)?;
        writeln!(
            output,
            "{}",
            json!({"operation":operation,"source":self.source(),"owned_pid":self.pid,"receipt":receipt})
        )?;
        Ok(())
    }

    pub(crate) fn event(&self, operation: &str) -> Result<()> {
        self.invoke(operation, None).map(|_| ())
    }

    pub(crate) fn invoke(&self, operation: &str, input: Option<&str>) -> Result<String> {
        let result = match operation {
            "clipboard-read" => return self.clipboard_read(),
            "clipboard-write" => {
                self.clipboard_write(input.context("Clipboard text required")?)?;
                return Ok(String::new());
            }
            "accessibility" | "print-cancel" => return self.specialized(operation, input),
            "script-dialog" => {
                let title = std::env::var("LIMO_CAD_SCRIPT_DIALOG_TITLE")
                    .context("LIMO_CAD_SCRIPT_DIALOG_TITLE")?;
                self.complete_dialog(&title, input.context("Script path required")?)?;
                return Ok(String::new());
            }
            "drawing-click" | "drawing-drag" | "drawing-pan" | "drawing-wheel" | "cam-row-drag" => {
                let request: Value = serde_json::from_str(input.context("Gesture required")?)?;
                self.gesture(operation, &request)?
            }
            "focus" => {
                let (activated, receipt) = super::keyboard::focus_target(
                    || self.observe_key_target(),
                    |observed, request| self.send(observed, request),
                )?;
                self.record(
                    "prepared-field-focus",
                    &json!({"owner":activated["owner"],"foreground":activated["foreground"],
                        "focused_control":activated["inspection"]["ui"]["focused_control"],
                        "foreground_window":foreground_diagnostics()}),
                )?;
                receipt
            }
            "pin-text-target" => {
                let observed = self.observe()?;
                let intended = super::keyboard::text_target(&observed)?;
                self.record("intended-text-target", &intended)?;
                *self.text_target.borrow_mut() = Some(intended);
                return Ok(String::new());
            }
            "diagnose-focus" => {
                // Read only. A setup failure can occur before the first key's
                // preparation, so preserve the foreign window at that point.
                let foreground = foreground_diagnostics();
                let observation = self.observe();
                self.record(
                    "field-focus-failure",
                    &json!({"foreground_window":foreground,
                        "observation":observation.as_ref().ok().map(|observed| json!({
                            "owner":observed["owner"],"foreground":observed["foreground"],
                            "focused_control":observed["inspection"]["ui"]["focused_control"]})),
                        "observation_error":observation.err().map(|error| format!("{error:#}"))}),
                )?;
                return Ok(String::new());
            }
            _ => {
                let key = match operation {
                    "select-all" => "Ctrl+A",
                    "copy" => "Ctrl+C",
                    "paste" => "Ctrl+V",
                    "right" => "ArrowRight",
                    "backspace" => "Backspace",
                    _ => bail!("Unknown Windows input operation {operation}"),
                };
                super::keyboard::send_key(
                    key,
                    self.text_target
                        .borrow()
                        .as_ref()
                        .context("Pin the intended editable field before sending a key")?,
                    || self.observe_key_target(),
                    |observed, request| {
                        if request["action"] == "focus" {
                            self.record(
                                "focus-before-key",
                                &json!({"observed_owner":observed["owner"],
                                    "foreground_window":foreground_diagnostics()}),
                            )?;
                        }
                        self.send(observed, request)
                    },
                )?
            }
        };
        self.record(operation, &result)?;
        Ok(serde_json::to_string(&result)?)
    }

    pub(crate) fn clipboard_read(&self) -> Result<String> {
        let mut clipboard = arboard::Clipboard::new()?;
        match clipboard.get_text() {
            Ok(text) => Ok(text),
            Err(arboard::Error::ContentNotAvailable) => Ok(String::new()),
            Err(error) => Err(error.into()),
        }
    }

    pub(crate) fn clipboard_write(&self, value: &str) -> Result<()> {
        {
            let mut clipboard = arboard::Clipboard::new()?;
            if value.is_empty() {
                clipboard.clear()?;
            } else {
                clipboard.set_text(value)?;
            }
        }
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

    fn gesture(&self, operation: &str, request: &Value) -> Result<Value> {
        let observed = self.observe()?;
        ensure!(
            observed["target_kind"] == "bevy_window",
            "A native modal blocks the CAD gesture"
        );
        let mapping = Mapping::new(&observed, &request["client"])?;
        let start = mapping.point(number(request, "x")?, number(request, "y")?)?;
        let mut input = json!({"point":start});
        let cancel = if operation == "cam-row-drag" {
            request["cancel"]
                .as_bool()
                .context("CAM cancel must be boolean")?
        } else {
            false
        };
        match operation {
            "drawing-click" => input["action"] = json!("click"),
            "drawing-wheel" => {
                let notches = request["notches"]
                    .as_i64()
                    .context("Wheel notches must be integral")?;
                ensure!(
                    notches != 0 && (-10..=10).contains(&notches),
                    "Wheel requires 1..10 signed notches"
                );
                input["action"] = json!("wheel");
                input["delta"] = json!(notches * 120);
                if request["ctrl"]
                    .as_bool()
                    .context("Wheel ctrl must be boolean")?
                {
                    input["modifiers"] = json!(["Ctrl"]);
                }
            }
            "drawing-pan" | "drawing-drag" => {
                input["action"] = json!("drag");
                input["button"] = json!(if operation == "drawing-pan" {
                    "middle"
                } else {
                    "left"
                });
                input["to"] =
                    json!(mapping.point(number(request, "to_x")?, number(request, "to_y")?)?);
            }
            "cam-row-drag" => {
                let points = request["points"]
                    .as_array()
                    .context("CAM waypoints required")?;
                ensure!(
                    (1..=8).contains(&points.len()),
                    "CAM requires 1..8 waypoints"
                );
                let mut hold_total = 0;
                let mut path = Vec::with_capacity(points.len());
                for point in points {
                    let hold = point["hold_ms"]
                        .as_u64()
                        .context("CAM hold must be integral")?;
                    ensure!(hold <= 800, "CAM hold exceeds 800 ms");
                    hold_total += hold;
                    ensure!(hold_total <= 1600, "CAM dwell exceeds 1600 ms");
                    path.push(json!({"point":mapping.point(number(point,"x")?, number(point,"y")?)?,"hold_ms":hold}));
                }
                input["action"] = json!("drag");
                input["path"] = json!(path);
                input["cancel"] = json!(cancel);
            }
            _ => bail!("Unknown CAD gesture {operation}"),
        }
        let receipt = self.send(&observed, input)?;
        let mut result = json!({"source":self.source(),"pid":self.pid,
            "window":observed["window_handle"],"operation":operation,"cancel":cancel,
            "client":request["client"],"scale":mapping.scale,"receipt":receipt,
            "coordinate_evidence":"Last cursor positions verified by the production input route; these do not prove product behavior"});
        for (side, key) in [
            ("start", "pointer_start_physical_client"),
            ("end", "pointer_end_physical_client"),
        ] {
            let point = receipt[key]
                .as_array()
                .context("Input receipt omitted verified pointer coordinates")?;
            ensure!(point.len() == 2, "Malformed verified pointer coordinates");
            let x = point[0].as_f64().context("Verified cursor x")?;
            let y = point[1].as_f64().context("Verified cursor y")?;
            result[format!("logical_{side}")] = json!([
                mapping.origin[0] + x / mapping.scale[0],
                mapping.origin[1] + y / mapping.scale[1]
            ]);
            result[format!("physical_{side}")] =
                json!([mapping.screen[0] + x, mapping.screen[1] + y]);
        }
        Ok(result)
    }

    pub(crate) fn complete_dialog(&self, title: &str, path: &str) -> Result<()> {
        ensure!(
            !title.is_empty() && !path.is_empty(),
            "Dialog title and path required"
        );
        ensure!(
            path.chars().count() <= 512 && !path.chars().any(char::is_control),
            "Dialog path requires 1-512 printable Unicode characters; no input was sent"
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        let observed = loop {
            let observed = self.observe()?;
            if observed["target_kind"] == "native_dialog" {
                require_dialog(&observed, title, None)?;
                break observed;
            }
            ensure!(Instant::now() < deadline, "Owned file dialog did not open");
            thread::sleep(Duration::from_millis(50));
        };
        let window = observed["window_handle"].as_u64().context("Dialog HWND")?;
        let native = &observed["inspection"]["native_dialog"];
        let focus = native["focused_control"]
            .as_u64()
            .context("Native focused control")?;
        ensure!(
            native["focused_editable"] == true,
            "The file dialog has no qualified focused text field"
        );
        let edit = native["controls"]
            .as_array()
            .context("Native controls")?
            .iter()
            .find(|control| control["window_handle"] == focus && control["enabled"] == true)
            .context("Focused file dialog edit control disappeared")?;
        let bounds = edit["screen_bounds"]
            .as_array()
            .context("Native edit bounds")?;
        ensure!(bounds.len() == 4, "Malformed native edit bounds");
        let rect = bounds
            .iter()
            .map(|v| v.as_f64().context("Native edit coordinate"))
            .collect::<Result<Vec<_>>>()?;
        ensure!(rect[2] > 0. && rect[3] > 0., "Native edit is not visible");
        let client = &observed["client_screen_bounds"];
        let point = [
            (rect[0] + rect[2] * 0.5 - number(client, "x")?).round(),
            (rect[1] + rect[3] * 0.5 - number(client, "y")?).round(),
        ];
        ensure!(
            point
                .iter()
                .all(|v| v.is_finite() && *v >= 0. && *v <= i32::MAX as f64),
            "Native edit coordinate is outside the owned client"
        );
        self.send(
            &observed,
            json!({"action":"click","point":[point[0] as i32,point[1] as i32]}),
        )?;
        for input in [
            json!({"action":"key","key":"Ctrl+A"}),
            json!({"action":"text","text":path}),
            json!({"action":"key","key":"Enter"}),
        ] {
            let observed = self.observe()?;
            require_dialog(&observed, title, Some(window))?;
            ensure!(
                observed["inspection"]["native_dialog"]["focused_control"] == focus
                    && observed["editable_focus"] == true,
                "The file dialog edit field lost focus; no further input was sent"
            );
            self.send(&observed, input)?;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let observed = self.observe()?;
            if observed["target_kind"] == "bevy_window" {
                return Ok(());
            }
            ensure!(
                observed["window_handle"] == window,
                "A different native modal appeared after file submission"
            );
            ensure!(
                Instant::now() < deadline,
                "File dialog remained open after Enter; inspect its visible error"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }

    /// Specialized hosted IME and UIA/print instrumentation retain their own evidence.
    pub(crate) fn command(&self, operation: &str) -> Result<Command> {
        self.check_process()?;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("platform");
        let (script, argument, window) = match operation {
            "ime-session" => {
                let observed = self.observe()?;
                ensure!(
                    observed["target_kind"] == "bevy_window"
                        && observed["presented"] == true
                        && observed["foreground"] == true,
                    "IME requires the owned presented foreground Bevy window"
                );
                (
                    self.helper.clone(),
                    "-ImeOwnedPid",
                    Some(
                        observed["main_window_handle"]
                            .as_u64()
                            .context("Owned main HWND")?,
                    ),
                )
            }
            "accessibility" => (
                root.join("native-accessibility-windows.ps1"),
                "-OwnedPid",
                None,
            ),
            "print-cancel" => (
                root.join("native-print-cancel-windows.ps1"),
                "-PrintOwnedPid",
                None,
            ),
            _ => bail!("No external helper exists for generic input operation {operation}"),
        };
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("powershell.exe");
        command.creation_flags(0x08000000);
        command
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(script)
            .arg(argument)
            .arg(self.pid.to_string());
        if let Some(window) = window {
            command.arg("-ImeWindow").arg(window.to_string());
        }
        command.env("LIMO_CAD_SESSION_DIR", &self.sessions);
        Ok(command)
    }

    fn specialized(&self, operation: &str, input: Option<&str>) -> Result<String> {
        let mut command = self.command(operation)?;
        let mut stdout = tempfile::tempfile()?;
        let mut stderr = tempfile::tempfile()?;
        let stdin = if let Some(input) = input {
            let mut file = tempfile::tempfile()?;
            file.write_all(input.as_bytes())?;
            file.seek(SeekFrom::Start(0))?;
            Stdio::from(file)
        } else {
            Stdio::null()
        };
        command
            .stdin(stdin)
            .stdout(Stdio::from(stdout.try_clone()?))
            .stderr(Stdio::from(stderr.try_clone()?));
        let mut child = command
            .spawn()
            .context("Start specialized Windows instrumentation")?;
        let deadline = Instant::now() + Duration::from_secs(45);
        while child.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("Specialized Windows instrumentation {operation} exceeded 45 seconds");
            }
            thread::sleep(Duration::from_millis(25));
        }
        let status = child.wait()?;
        ensure!(
            status.success(),
            "Specialized Windows instrumentation {operation} failed: {}",
            output(&mut stderr)?
        );
        self.check_process()?;
        output(&mut stdout)
    }
}

// Diagnostic identity of the window that took foreground. It never qualifies
// that window for input or permits closing it.
fn foreground_diagnostics() -> Value {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
    };
    let hwnd = unsafe { GetForegroundWindow() };
    let mut pid = 0;
    let mut title = [0u16; 512];
    let mut class = [0u16; 512];
    let (title_len, class_len) = unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        (
            GetWindowTextW(hwnd, &mut title),
            GetClassNameW(hwnd, &mut class),
        )
    };
    let process = match process_identity(pid) {
        Ok((image, start_time)) => json!({"executable":image,"start_time":start_time}),
        Err(error) => json!({"identity_error":format!("{error:#}")}),
    };
    json!({"diagnostics_only":true,"hwnd":hwnd.0 as usize,"pid":pid,
        "title":String::from_utf16_lossy(&title[..title_len.max(0) as usize]),
        "class":String::from_utf16_lossy(&class[..class_len.max(0) as usize]),
        "process":process,"still_foreground":unsafe { GetForegroundWindow() == hwnd }})
}

// Runner prompts can take foreground after startup. Prepare only individually
// qualified disposable-runner windows, then use the unchanged input guards.
fn prepare_hosted_arm_desktop(out: &Path) -> Result<()> {
    prepare_hosted_arm_desktop_at(&out.join("runner-ready-desktop.json"), None)
}

fn hosted_arm_desktop() -> bool {
    std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
        && std::env::var("RUNNER_ARCH").as_deref() == Ok("ARM64")
}

fn prepare_hosted_arm_desktop_at(evidence: &Path, window: Option<u64>) -> Result<()> {
    if !hosted_arm_desktop() {
        return Ok(());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("Repository root")?;
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("pwsh.exe");
    command
        .creation_flags(0x08000000)
        .args(["-NoProfile", "-NonInteractive", "-File"])
        .arg(root.join("scripts/prepare-hosted-arm-desktop.ps1"))
        .arg("-EvidencePath")
        .arg(evidence)
        .stdin(Stdio::null());
    if let Some(window) = window {
        command.arg("-Window").arg(window.to_string());
    }
    let output = command
        .output()
        .context("Prepare the disposable ARM64 input desktop")?;
    ensure!(
        output.status.success(),
        "ARM64 ready-desktop preparation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

struct Mapping {
    origin: [f64; 2],
    scale: [f64; 2],
    size: [f64; 2],
    screen: [f64; 2],
}

impl Mapping {
    fn new(observed: &Value, requested: &Value) -> Result<Self> {
        ensure!(
            observed["presented"] == true,
            "CAD has no presented client coordinates"
        );
        let client = &observed["inspection"]["ui"]["client"];
        for key in ["x", "y", "width", "height"] {
            ensure!(
                (number(client, key)? - number(requested, key)?).abs() <= 0.01,
                "Gesture client bounds changed; inspect and plan again"
            );
        }
        let scale = &observed["interface_to_physical_scale"];
        let scale = [
            scale[0].as_f64().context("Client scale x")?,
            scale[1].as_f64().context("Client scale y")?,
        ];
        ensure!(
            scale.iter().all(|value| value.is_finite() && *value > 0.),
            "Invalid client scale"
        );
        let physical = &observed["client_screen_bounds"];
        let size = [number(physical, "width")?, number(physical, "height")?];
        ensure!(
            size.iter()
                .all(|value| *value > 0. && *value <= i32::MAX as f64),
            "Invalid physical client size"
        );
        Ok(Self {
            origin: [number(client, "x")?, number(client, "y")?],
            scale,
            size,
            screen: [number(physical, "x")?, number(physical, "y")?],
        })
    }

    fn point(&self, x: f64, y: f64) -> Result<[i32; 2]> {
        let values = [
            (x - self.origin[0]) * self.scale[0],
            (y - self.origin[1]) * self.scale[1],
        ];
        ensure!(
            values
                .iter()
                .enumerate()
                .all(|(axis, value)| value.is_finite()
                    && *value >= 0.
                    && value.round() < self.size[axis]),
            "Gesture point lies outside the owned CAD client"
        );
        Ok([values[0].round() as i32, values[1].round() as i32])
    }
}

fn require_dialog(observed: &Value, title: &str, window: Option<u64>) -> Result<()> {
    ensure!(
        observed["target_kind"] == "native_dialog" && observed["presented"] == true,
        "Owned file dialog is no longer presented"
    );
    if let Some(window) = window {
        ensure!(
            observed["window_handle"] == window,
            "File dialog was replaced"
        );
    }
    let window = &observed["window_handle"];
    let diagnostics = observed["native_window_diagnostics"]["windows"]
        .as_array()
        .context("Native window titles unavailable")?;
    ensure!(
        diagnostics
            .iter()
            .any(|item| &item["hwnd"] == window && item["caption"] == title),
        "The owned native modal does not match requested title {title}"
    );
    Ok(())
}

fn number(value: &Value, key: &str) -> Result<f64> {
    value[key]
        .as_f64()
        .filter(|value| value.is_finite())
        .with_context(|| format!("Finite {key} required"))
}

fn process_identity(pid: u32) -> Result<(PathBuf, u64)> {
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    let process = system
        .process(pid)
        .context("Owned CAD process is no longer running")?;
    let image = process
        .exe()
        .context("Cannot read owned CAD executable")?
        .canonicalize()?;
    let start_time = process.start_time();
    ensure!(start_time > 0, "Cannot retain owned CAD process start time");
    Ok((image, start_time))
}

fn private_sessions(out: &Path, pid: u32, image: &Path) -> Result<PathBuf> {
    let out = out.canonicalize()?;
    let root = if out.join("sessions/_ui/processes").is_dir() {
        out.clone()
    } else {
        out.ancestors()
            .find(|root| {
                root.join("host.json").is_file() && root.join("sessions/_ui/processes").is_dir()
            })
            .context("Input requires an explicit private fixture session registry")?
            .to_owned()
    };
    let sessions = root.join("sessions").canonicalize()?;
    crate::package::ordinary_directory(&sessions)?;
    if let Some(override_path) = std::env::var_os("LIMO_CAD_SESSION_DIR") {
        ensure!(
            PathBuf::from(override_path).canonicalize()? == sessions,
            "The inherited session registry differs from the owned fixture registry"
        );
    }
    if root.join("host.json").is_file() {
        let launch: Value = serde_json::from_slice(&fs::read(root.join("host.json"))?)?;
        ensure!(
            launch["pid"] == pid
                && Path::new(launch["exe"].as_str().context("Fixture executable")?)
                    .canonicalize()?
                    == image,
            "Fixture launch record does not match the owned CAD process"
        );
    }
    Ok(sessions)
}

fn active_lease(sessions: &Path, pid: u32) -> Result<Lease> {
    let now = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    let mut leases = Vec::new();
    for entry in fs::read_dir(sessions.join("_ui/processes"))? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let parsed: Value = serde_json::from_slice(&fs::read(path)?)?;
        if parsed["pid"] != pid {
            continue;
        }
        let updated = parsed["updated_ms"]
            .as_u64()
            .context("Process lease timestamp")?;
        ensure!(
            now.saturating_sub(updated) <= 90_000,
            "Owned CAD lease expired"
        );
        let windows = parsed["windows"]
            .as_array()
            .context("Owned window inventory")?;
        ensure!(
            windows.len() == 1,
            "Fixture input requires one unambiguous owned CAD window"
        );
        let window = &windows[0];
        leases.push(Lease {
            process_instance: parsed["process_instance_id"]
                .as_str()
                .context("Process instance")?
                .to_owned(),
            window_id: window["window_id"]
                .as_str()
                .context("Window identity")?
                .to_owned(),
            session: window["active_session_id"]
                .as_str()
                .context("Active session identity")?
                .to_owned(),
            document: window["active_document_id"]
                .as_str()
                .context("Active document identity")?
                .to_owned(),
        });
    }
    ensure!(
        leases.len() == 1,
        "Owned CAD process must have one unique live lease"
    );
    leases.pop().context("Owned CAD process lease absent")
}

fn output(file: &mut fs::File) -> Result<String> {
    String::from_utf8(super::helper_output(file)?).context("Instrumentation output was not UTF-8")
}
