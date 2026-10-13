//! Disposable Japanese input against the actual owned Bevy editor.
//! OS keys are the only source of preedit/commit; MCP observes and captures.
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, Stdio},
    sync::mpsc::{self, Receiver},
    time::{SystemTime, UNIX_EPOCH},
};

const SOURCE: &str = "com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese";
const TEXT: &str = "はる";
pub(super) const OPT_IN: &str = if cfg!(target_os = "windows") {
    "windows-japanese"
} else {
    "macos-japanese"
};

fn selected_source() -> &'static str {
    if cfg!(target_os = "windows") {
        windows_ime::SOURCE
    } else {
        SOURCE
    }
}

pub(super) fn guard() -> Result<()> {
    if cfg!(target_os = "windows") {
        return windows_ime::guard();
    }
    ensure!(
        hosted::enabled(
            std::env::consts::OS,
            "macos",
            "macOS",
            ("LIMO_CAD_NATIVE_IME_TEST", "macos-japanese"),
            |key| std::env::var(key).ok(),
        ),
        "Japanese IME input requires the explicit disposable GitHub macOS runner"
    );
    Ok(())
}

fn hash(path: &Path) -> Result<String> {
    if cfg!(target_os = "windows") {
        return windows_ime::hash(path);
    }
    crate::hash::file(path)
}

pub(super) fn prerequisite(path: &Path, out: &Path) -> Result<Value> {
    guard()?;
    let root = PathBuf::from(std::env::var_os("RUNNER_TEMP").context("RUNNER_TEMP is absent")?)
        .canonicalize()?;
    let out = out.canonicalize()?;
    let path = path.canonicalize()?;
    ensure!(
        out.starts_with(&root) && out != root && path.starts_with(&root),
        "IME evidence and stock prerequisite must be beneath RUNNER_TEMP"
    );
    ensure!(
        fs::metadata(&path)?.len() <= 2 * 1024 * 1024,
        "Stock report exceeds evidence budget"
    );
    let bytes = fs::read(&path)?;
    let report: Value =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))?;
    let run = std::env::var("GITHUB_RUN_ID")?;
    let succeeded = if cfg!(target_os = "windows") {
        windows_ime::stock_success(&report, &run)
    } else {
        stock_success(&report, &run)
    };
    ensure!(succeeded,
        "Expected a passed, fully restored stock IME prerequisite from this disposable job: {report}");
    fs::write(
        out.join("ime-stock-prerequisite.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    Ok(
        json!({"path":path, "sha256":hash(&path)?, "run_id":report["environment"]["run_id"],
        "stock_native_bevy_validated":if cfg!(target_os = "windows") { &report["ime"]["native_bevy_validated"] } else { &report["native_bevy_validated"] }}),
    )
}

fn stock_success(report: &Value, run_id: &str) -> bool {
    report["status"] == "stock-control-ime-feasible"
        && report["child_exit_code"] == 0
        && report["process_exit_code"] == 0
        && report["cleanup"]["enabled_set_restored"] == true
        && report["cleanup"]["errors"]
            .as_array()
            .is_some_and(Vec::is_empty)
        && report["native_bevy_validated"] == false
        && report["final_field"]["committed"] == TEXT
        && report["final_field"]["has_marked_text"] == false
        && report["environment"]["run_id"] == run_id
}

struct Session {
    child: Child,
    input: Option<ChildStdin>,
    replies: Receiver<Result<Value, String>>,
    sequence: u64,
    session: String,
    field: Value,
    field_token: String,
    window_number: u64,
}

impl Session {
    fn start(
        driver: &Driver,
        server: &Path,
        out: &Path,
        session: &str,
        field: &Value,
    ) -> Result<Self> {
        let log = fs::File::create(out.join("japanese-ime-driver.stderr.log"))?;
        let field_token = format!("{}:{}", field["control_key"], field["binding"]);
        let mut child = driver
            .command("ime-session")?
            .env("LIMO_CAD_IME_HOST_PATH", server)
            .env("LIMO_CAD_IME_OUT", out.canonicalize()?)
            .env("LIMO_CAD_IME_SESSION", session)
            .env("LIMO_CAD_IME_FIELD_TOKEN", &field_token)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(log))
            .spawn()?;
        let input = child.stdin.take();
        let stdout = child.stdout.take().context("IME helper stdout")?;
        let (sender, replies) = mpsc::sync_channel(16);
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let parsed = line.map_err(|e| e.to_string()).and_then(|line| {
                    if line.len() > 65536 {
                        return Err("IME reply exceeds byte budget".into());
                    }
                    serde_json::from_str(&line).map_err(|e| e.to_string())
                });
                if sender.send(parsed).is_err() {
                    break;
                }
            }
        });
        let mut result = Self {
            child,
            input,
            replies,
            sequence: 0,
            session: session.to_owned(),
            field: field.clone(),
            field_token,
            window_number: 0,
        };
        let ready = result.receive()?;
        ensure!(
            ready["status"] == "ready" && ready["source_id"] == selected_source(),
            "IME helper did not acknowledge the owned window: {ready}"
        );
        result.window_number = ready["window_number"]
            .as_u64()
            .filter(|id| *id > 0)
            .context("Missing owned OS window identity")?;
        Ok(result)
    }

    fn receive(&mut self) -> Result<Value> {
        self.replies.recv_timeout(Duration::from_secs(10))
            .context("Timed out or closed IME helper reply; inspect japanese-ime-driver.stderr.log and cleanup report")?
            .map_err(anyhow::Error::msg)
    }

    fn request(&mut self, operation: &str, client: &mut Client) -> Result<Value> {
        let inspected = ui(client, json!({"action":"inspect"}))?;
        ensure!(
            inspected["active_session_id"] == self.session && focus(&inspected)? == &self.field,
            "Owned Rename field/session changed before IME input: {inspected}"
        );
        self.sequence += 1;
        let request = json!({"operation":operation, "sequence":self.sequence,
            "session":self.session, "field_token":self.field_token,
            "focused_control":inspected["ui"]["focused_control"],
            "checked_unix_ms":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs_f64() * 1000.0});
        writeln!(
            self.input.as_mut().context("IME helper stdin closed")?,
            "{request}"
        )?;
        let reply = self.receive()?;
        ensure!(
            reply["status"] == "applied"
                && reply["sequence"] == self.sequence
                && reply["operation"] == operation
                && if cfg!(target_os = "windows") {
                    reply["language"] == 0x411
                } else {
                    reply["selected_source"] == SOURCE
                },
            "IME OS operation failed: {reply}"
        );
        Ok(reply)
    }

    fn finish(&mut self) -> Result<Value> {
        self.sequence += 1;
        writeln!(
            self.input.as_mut().context("IME helper stdin closed")?,
            "{}",
            json!({"operation":"finish", "sequence":self.sequence})
        )?;
        drop(self.input.take());
        let reply = self.receive()?;
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = self.child.try_wait()? {
                break status;
            }
            ensure!(
                Instant::now() < deadline,
                "IME helper did not exit after cleanup"
            );
            thread::sleep(Duration::from_millis(25));
        };
        ensure!(
            status.success()
                && reply["status"] == "finished"
                && reply["result"] == "passed"
                && if cfg!(target_os = "windows") {
                    reply["cleanup"]["layout_restored"] == true
                } else {
                    reply["cleanup"]["enabled_set_restored"] == true
                }
                && reply["cleanup"]["errors"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
            "IME helper did not restore the source set exactly: {reply}"
        );
        Ok(reply)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.input.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().ok().flatten().is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(25));
        }
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn trace(snapshot: &Value) -> Result<&Value> {
    let trace = &snapshot["ui"]["ime_diagnostics"];
    ensure!(
        trace["overflow"] == false && trace["events"].is_array(),
        "Missing or overflowed real Bevy IME diagnostics: {trace}"
    );
    Ok(trace)
}

fn focus(snapshot: &Value) -> Result<&Value> {
    let focus = &trace(snapshot)?["focus"];
    ensure!(
        focus["control_key"].is_u64()
            && focus["binding"].is_u64()
            && focus["label"] == "Project name"
            && controls(snapshot).any(|control| control["id"] == snapshot["ui"]["focused_control"]
                && control["label"] == "Project name"),
        "Owned Rename focus is absent: {snapshot}"
    );
    Ok(focus)
}

fn event_context_matches(current: &Value, field: &Value, window_number: u64) -> bool {
    if cfg!(target_os = "windows") {
        return windows_ime::event_context_matches(current, field, window_number);
    }
    appkit_event_context_matches(current, field, window_number)
}

fn appkit_event_context_matches(current: &Value, field: &Value, window_number: u64) -> bool {
    current["context"].is_object()
        && current["context"] == field["context"]
        && current["control_label"] == "Project name"
        && current["control_key"] == field["control_key"]
        && current["binding"] == field["binding"]
        && current["appkit"]["source_id"] == SOURCE
        && current["appkit"]["window_number"] == window_number
        && current["appkit"]["key_window"] == true
        && current["appkit"]["first_responder_is_view"] == true
}

fn native_context_ready(current: &Value, field: &Value, window_number: u64) -> bool {
    if cfg!(target_os = "windows") {
        return windows_ime::native_context_ready(current, field, window_number);
    }
    appkit_native_context_ready(current, field, window_number)
}

fn appkit_native_context_ready(current: &Value, field: &Value, window_number: u64) -> bool {
    current["context"].is_object()
        && current["context"] == field["context"]
        && current["control_key"] == field["control_key"]
        && current["binding"] == field["binding"]
        && current["window"]["ime_enabled"] == true
        && current["appkit"]["source_id"] == SOURCE
        && current["appkit"]["window_number"] == window_number
        && current["appkit"]["key_window"] == true
        && current["appkit"]["first_responder_is_view"] == true
        && current["appkit"]["input_context_present"] == true
        && [
            "native_text_field",
            "editable_text",
            "computed_node",
            "ui_transform",
            "render_target",
        ]
        .iter()
        .all(|component| current["native_field_components"][*component] == true)
}

fn wait_state(
    client: &mut Client,
    input: &Session,
    after: u64,
    expected: impl Fn(&Value, &Value) -> bool,
) -> Result<Value> {
    let session = input.session.as_str();
    let field = &input.field;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = ui(client, json!({"action":"inspect"}))?;
        ensure!(
            snapshot["active_session_id"] == session && focus(&snapshot)? == field,
            "IME confirmation submitted or blurred Rename: {snapshot}"
        );
        let current = &trace(&snapshot)?["current"];
        if current["sequence"].as_u64().is_some_and(|seq| seq > after)
            && expected(current, &snapshot)
        {
            ensure!(
                event_context_matches(current, field, input.window_number),
                "Received IME event has wrong owner/input context: {current}"
            );
            return Ok(snapshot);
        }
        ensure!(
            Instant::now() < deadline,
            "Expected real Bevy IME event did not arrive: {snapshot}"
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn commits(snapshot: &Value, after: u64) -> Result<Vec<Value>> {
    Ok(trace(snapshot)?["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| {
            event["sequence"].as_u64().is_some_and(|seq| seq > after) && event["kind"] == "commit"
        })
        .cloned()
        .collect())
}

fn owned_commits(snapshot: &Value, after: u64, field: &Value, window: u64) -> Result<Vec<Value>> {
    let accepted = commits(snapshot, after)?;
    ensure!(
        accepted
            .iter()
            .all(|event| event_context_matches(event, field, window) && event["value"].is_string()),
        "A received Commit has the wrong document, field, window, or input source: {accepted:?}"
    );
    Ok(accepted
        .into_iter()
        .filter(|event| event["value"] != "")
        .collect())
}

fn unchanged_draft(before: &Value, after: &Value) -> bool {
    before["value"].is_string()
        && before["selection"]["start"].is_u64()
        && before["selection"]["end"].is_u64()
        && before["value"] == after["value"]
        && before["selection"] == after["selection"]
}

fn retain(out: &Path, name: &str, value: &Value) -> Result<()> {
    fs::write(
        out.join(format!("{name}.json")),
        serde_json::to_vec_pretty(value)?,
    )?;
    Ok(())
}

fn exercise_selected_cancellation(
    client: &mut Client,
    driver: &Driver,
    input: &mut Session,
    out: &Path,
    baseline: u64,
    accepted: &[Value],
    project: &Value,
) -> Result<(Value, Vec<Value>)> {
    driver.event("select-all")?;
    let before = wait_field(client, |field| {
        field["value"] == TEXT && selected_all(field, TEXT)
    })?;
    retain(out, "ime-selection-before", &before)?;
    let initial = ui(client, json!({"action":"inspect"}))?;
    let sequence = trace(&initial)?["current"]["sequence"].as_u64().unwrap();
    input.request("preedit", client)?;
    let preedit = wait_state(client, input, sequence, |current, _| {
        current["kind"] == "preedit" && current["value"] == TEXT && current["composing"] == true
    })?;
    retain(out, "ime-selection-preedit", &preedit)?;
    ensure!(
        text_state(client)?["value"] == ""
            && owned_commits(&preedit, baseline, &input.field, input.window_number)? == accepted,
        "Selected preedit did not replace exactly the selected draft provisionally"
    );
    ensure!(
        &client.call("cad_project_model", json!({}))? == project,
        "Selected preedit changed the project"
    );

    let sequence = trace(&preedit)?["current"]["sequence"].as_u64().unwrap();
    input.request("escape", client)?;
    let first = wait_state(client, input, sequence, |current, _| {
        current["composing"] == false || (current["kind"] == "preedit" && current["value"] == TEXT)
    })?;
    retain(out, "ime-selection-first-escape", &first)?;
    ensure!(
        owned_commits(&first, baseline, &input.field, input.window_number)? == accepted,
        "First Escape inserted text over the original selection"
    );
    let (cancelled, escape_count) = if trace(&first)?["current"]["composing"] == true {
        let sequence = trace(&first)?["current"]["sequence"].as_u64().unwrap();
        input.request("escape", client)?;
        (
            wait_state(client, input, sequence, |current, _| {
                current["composing"] == false
            })?,
            2,
        )
    } else {
        (first, 1)
    };
    retain(out, "ime-selection-cancelled", &cancelled)?;
    let field = text_state(client)?;
    retain(out, "ime-selection-cancelled-field", &field)?;
    ensure!(
        unchanged_draft(&before, &field)
            && owned_commits(&cancelled, baseline, &input.field, input.window_number)? == accepted,
        "Selected cancellation changed accepted text/selection or inserted text: {field}"
    );
    ensure!(
        &client.call("cad_project_model", json!({}))? == project,
        "Selected cancellation changed the project"
    );
    capture(client, out, "ime-selection-cancelled")?;

    let sequence = trace(&cancelled)?["current"]["sequence"].as_u64().unwrap();
    input.request("preedit", client)?;
    let replacement_preedit = wait_state(client, input, sequence, |current, _| {
        current["kind"] == "preedit" && current["value"] == TEXT && current["composing"] == true
    })?;
    retain(
        out,
        "ime-selection-replacement-preedit",
        &replacement_preedit,
    )?;
    ensure!(
        text_state(client)?["value"] == "",
        "Replacement preedit did not own the restored selection"
    );
    let sequence = trace(&replacement_preedit)?["current"]["sequence"]
        .as_u64()
        .unwrap();
    input.request("commit", client)?;
    let end = TEXT.encode_utf16().count();
    let replacement = wait_state(client, input, sequence, |current, snapshot| {
        current["composing"] == false
            && controls(snapshot).any(|field| {
                field["label"] == "Project name"
                    && field["value"] == TEXT
                    && field["selection"] == json!({"start":end,"end":end})
            })
            && owned_commits(snapshot, baseline, &input.field, input.window_number)
                .is_ok_and(|events| events.len() == accepted.len() + 1)
    })?;
    retain(out, "ime-selection-replacement-committed", &replacement)?;
    let commits = owned_commits(&replacement, baseline, &input.field, input.window_number)?;
    ensure!(
        commits[..accepted.len()] == *accepted && commits.last().unwrap()["value"] == TEXT,
        "Identical subsequent Commit was dropped or reordered: {commits:?}"
    );
    ensure!(
        &client.call("cad_project_model", json!({}))? == project,
        "Replacement IME Commit submitted the form or changed the project"
    );
    Ok((
        json!({"before":before,"preedit":preedit,"cancelled":cancelled,"field":field,
        "escape_count":escape_count,"replacement_committed":replacement}),
        commits,
    ))
}

pub(super) fn exercise(
    client: &mut Client,
    driver: &Driver,
    server: &Path,
    out: &Path,
    session: &str,
    original: &str,
    stock: &Value,
) -> Result<Value> {
    guard()?;
    let project = client.call("cad_project_model", json!({}))?;
    retain(out, "ime-project-before", &project)?;
    let initial = ui(client, json!({"action":"inspect"}))?;
    retain(out, "ime-initial", &initial)?;
    if cfg!(target_os = "windows") {
        windows_ime::validate_initial(&initial, client.process_id())?;
    }
    let baseline = trace(&initial)?["current"]["sequence"]
        .as_u64()
        .unwrap_or(0);
    let field = focus(&initial)?.clone();
    driver.event("select-all")?;
    driver.event("backspace")?;
    wait_field(client, |field| field["value"] == "")?;
    let mut input = Session::start(driver, server, out, session, &field)?;
    input.request("enable", client)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = ui(client, json!({"action":"inspect"}))?;
        retain(out, "ime-before-preedit", &snapshot)?;
        ensure!(
            snapshot["active_session_id"] == session && focus(&snapshot)? == &field,
            "Owned Rename field changed before native input context became ready: {snapshot}"
        );
        if native_context_ready(
            &trace(&snapshot)?["configuration"],
            &field,
            input.window_number,
        ) {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Owned Bevy input context is not ready; no preedit keys sent: {snapshot}"
        );
        thread::sleep(Duration::from_millis(50));
    }
    input.request("preedit", client)?;
    let preedit = wait_state(client, &input, baseline, |current, _| {
        current["kind"] == "preedit" && current["value"] == TEXT && current["composing"] == true
    })?;
    ensure!(
        text_state(client)?["value"] == "" && commits(&preedit, baseline)?.is_empty(),
        "Preedit prematurely inserted committed text"
    );
    retain(out, "ime-preedit", &preedit)?;
    capture(client, out, "ime-preedit")?;
    ensure!(
        client.call("cad_project_model", json!({}))? == project,
        "Preedit changed the project"
    );

    let before_commit = trace(&preedit)?["current"]["sequence"].as_u64().unwrap();
    input.request("commit", client)?;
    let committed = wait_state(client, &input, before_commit, |current, snapshot| {
        current["composing"] == false
            && controls(snapshot)
                .any(|control| control["label"] == "Project name" && control["value"] == TEXT)
            && commits(snapshot, baseline)
                .is_ok_and(|events| events.len() == 1 && events[0]["value"] == TEXT)
    })?;
    let committed_field = text_state(client)?;
    retain(out, "ime-committed-field", &committed_field)?;
    ensure!(
        committed_field["value"] == TEXT,
        "Return did not commit exact Hiragana text"
    );
    let mut accepted = owned_commits(&committed, baseline, &input.field, input.window_number)?;
    ensure!(
        accepted.len() == 1 && accepted[0]["value"] == TEXT,
        "Expected exactly one real Bevy Commit: {accepted:?}"
    );
    ensure!(
        client.call("cad_project_model", json!({}))? == project,
        "IME Return submitted Rename or changed the project"
    );
    retain(out, "ime-committed", &committed)?;
    capture(client, out, "ime-committed")?;

    let committed_sequence = trace(&committed)?["current"]["sequence"].as_u64().unwrap();
    input.request("preedit", client)?;
    let second = wait_state(client, &input, committed_sequence, |current, _| {
        current["kind"] == "preedit" && current["value"] == TEXT && current["composing"] == true
    })?;
    ensure!(
        text_state(client)?["value"] == TEXT,
        "Second preedit changed committed text"
    );
    retain(out, "ime-second-preedit", &second)?;
    let second_sequence = trace(&second)?["current"]["sequence"].as_u64().unwrap();
    input.request("escape", client)?;
    let first_escape = wait_state(client, &input, second_sequence, |current, _| {
        current["composing"] == false || (current["kind"] == "preedit" && current["value"] == TEXT)
    })?;
    ensure!(
        owned_commits(&first_escape, baseline, &input.field, input.window_number)? == accepted
            && text_state(client)?["value"] == TEXT,
        "First Escape changed accepted text or inserted a commit"
    );
    retain(out, "ime-first-escape", &first_escape)?;
    let (cancelled, escape_count) = if trace(&first_escape)?["current"]["composing"] == true {
        let after = trace(&first_escape)?["current"]["sequence"]
            .as_u64()
            .unwrap();
        input.request("escape", client)?;
        (
            wait_state(client, &input, after, |current, _| {
                current["composing"] == false
            })?,
            2,
        )
    } else {
        (first_escape, 1)
    };
    retain(out, "ime-cancelled", &cancelled)?;
    let cancelled_commits = owned_commits(&cancelled, baseline, &input.field, input.window_number)?;
    let cancelled_field = text_state(client)?;
    retain(out, "ime-cancelled-field", &cancelled_field)?;
    ensure!(
        cancelled_commits == accepted && unchanged_draft(&committed_field, &cancelled_field),
        "Escape inserted text or changed the accepted draft/selection: commits={cancelled_commits:?}, field={cancelled_field}"
    );
    ensure!(
        client.call("cad_project_model", json!({}))? == project,
        "IME Escape changed the project"
    );
    capture(client, out, "ime-cancelled")?;
    let (selected_cancellation, replacement_commits) = exercise_selected_cancellation(
        client, driver, &mut input, out, baseline, &accepted, &project,
    )?;
    accepted = replacement_commits;
    let cleanup = input.finish()?;
    if cfg!(target_os = "windows") {
        windows_ime::check_restored(client, &initial, out)?;
    }
    driver.event("select-all")?;
    let selected = wait_field(client, |field| {
        field["value"] == TEXT && selected_all(field, TEXT)
    })?;
    retain(out, "ime-cancelled-selected", &selected)?;
    driver.clipboard_write(original)?;
    driver.event("paste")?;
    let restored_field = wait_field(client, |field| field["value"] == original)?;
    let final_trace = ui(client, json!({"action":"inspect"}))?;
    retain(out, "ime-final", &final_trace)?;
    let final_field = text_state(client)?;
    retain(out, "ime-final-field", &final_field)?;
    ensure!(
        owned_commits(&final_trace, baseline, &field, input.window_number)? == accepted
            && unchanged_draft(&restored_field, &final_field),
        "Late IME delivery changed the restored draft/selection or inserted text: {final_field}"
    );
    ensure!(
        client.call("cad_project_model", json!({}))? == project,
        "Restoring the field changed the project"
    );
    let report = json!({"engine":if cfg!(target_os = "windows") { "Microsoft Japanese Romaji/Hiragana" } else { "Apple Japanese Romaji/Hiragana" }, "source_id":selected_source(),
        "event_source":driver.source(),
        "stock_prerequisite":stock, "host_sha256":hash(server)?, "driver_sha256":if cfg!(target_os = "windows") {
            hash(&std::env::current_exe()?)?
        } else { hash(&driver.helper)? },
        "windows_session_helper_sha256":if cfg!(target_os = "windows") {
            Some(hash(&driver.helper)?)
        } else { None },
        "preedit":preedit, "committed":committed, "second_preedit":second, "cancelled":cancelled,
        "escape_count":escape_count, "selected_cancellation":selected_cancellation,
        "post_cancel_selection":selected, "cleanup":cleanup,
        "exact_project_unchanged":true, "candidate_popup_pixels_validated":false,
        "not_tested":["candidate popup placement/pixels", "physical keyboard", "monitor DPI transitions"]});
    retain(out, "ime-result", &report)?;
    Ok(report)
}

pub(super) fn cancel_and_check(client: &mut Client, out: &Path) -> Result<()> {
    let before: Value = serde_json::from_slice(&fs::read(out.join("ime-project-before.json"))?)?;
    control(client, "Cancel", None)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snapshot = ui(client, json!({"action":"inspect"}))?;
        if !controls(&snapshot).any(|control| control["label"] == "Project name") {
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "Rename did not close after explicit Cancel"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let after = client.call("cad_project_model", json!({}))?;
    retain(out, "ime-project-after-cancel", &after)?;
    ensure!(
        after == before,
        "IME workflow changed the exact project after explicit Cancel"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_final_preedit_does_not_bless_a_foreign_commit() {
        let field = json!({"control_key":17,"binding":2,
            "context":{"document_id":"doc","window_id":"main","epoch":3}});
        let preedit = json!({"sequence":2,"kind":"preedit","value":"","context":field["context"],
            "control_key":17,"binding":2,"control_label":"Project name",
            "appkit":{"source_id":SOURCE,"window_number":32,"key_window":true,"first_responder_is_view":true},
            "win32":{"window":32,"pid":91,"window_thread":92,"foreground":true,"focused":true,"language":0x411,
                "active_profile":{"type":1,"language":0x411,"class_id":"03b5835f-f03c-411b-9ce2-aa23e1171e36",
                    "profile_id":windows_ime::SOURCE}}});
        let mut commit = preedit.clone();
        commit["sequence"] = json!(1);
        commit["kind"] = json!("commit");
        commit["value"] = json!(TEXT);
        commit["context"]["document_id"] = json!("wrong-document");
        let snapshot = json!({"ui":{"ime_diagnostics":{"overflow":false,"current":preedit,
            "events":[commit,preedit]}}});
        assert!(event_context_matches(
            &snapshot["ui"]["ime_diagnostics"]["current"],
            &field,
            32
        ));
        assert_eq!(commits(&snapshot, 0).unwrap().len(), 1);
        assert!(owned_commits(&snapshot, 0, &field, 32).is_err());
        let mut valid = snapshot;
        valid["ui"]["ime_diagnostics"]["events"][0]["context"] = field["context"].clone();
        assert_eq!(owned_commits(&valid, 0, &field, 32).unwrap().len(), 1);
        valid["ui"]["ime_diagnostics"]["events"][0]["value"] = json!("");
        assert_eq!(commits(&valid, 0).unwrap().len(), 1);
        assert!(owned_commits(&valid, 0, &field, 32).unwrap().is_empty());
        valid["ui"]["ime_diagnostics"]["events"][0]["context"]["epoch"] = json!(99);
        assert!(owned_commits(&valid, 0, &field, 32).is_err());
    }

    #[test]
    fn cancellation_requires_exact_draft_and_selection_even_with_empty_os_commit() {
        let before = json!({"value":TEXT,"selection":{"start":0,"end":2}});
        assert!(unchanged_draft(&before, &before));
        for after in [
            json!({"value":"","selection":{"start":0,"end":0}}),
            json!({"value":TEXT,"selection":{"start":2,"end":2}}),
            json!({"value":TEXT,"selection":null}),
        ] {
            assert!(!unchanged_draft(&before, &after));
        }
    }

    #[test]
    fn event_owner_is_the_entire_document_context_not_the_session_id() {
        let field = json!({"control_key":17,"binding":2,
            "context":{"document_id":"document-not-session","window_id":"main","epoch":3}});
        let current = json!({"control_key":17,"binding":2,"control_label":"Project name",
            "context":field["context"], "appkit":{"source_id":SOURCE,"window_number":32,
            "key_window":true,"first_responder_is_view":true}});
        assert!(appkit_event_context_matches(&current, &field, 32));
        for key in ["document_id", "window_id", "epoch"] {
            let mut stale = current.clone();
            stale["context"][key] = json!("different");
            assert!(!appkit_event_context_matches(&stale, &field, 32));
        }
        assert!(!appkit_event_context_matches(&current, &field, 33));
    }

    #[test]
    fn native_preedit_requires_enabled_complete_editor_and_actual_appkit_source() {
        let field = json!({"control_key":17,"binding":2,
            "context":{"document_id":"doc","window_id":"main","epoch":3}});
        let ready = json!({"control_key":17,"binding":2,"context":field["context"],
            "window":{"ime_enabled":true}, "appkit":{"source_id":SOURCE,"window_number":32,
                "key_window":true,"first_responder_is_view":true,"input_context_present":true},
            "native_field_components":{"native_text_field":true,"editable_text":true,
                "computed_node":true,"ui_transform":true,"render_target":true}});
        assert!(appkit_native_context_ready(&ready, &field, 32));
        for pointer in [
            "/window/ime_enabled",
            "/appkit/source_id",
            "/appkit/first_responder_is_view",
            "/appkit/input_context_present",
            "/native_field_components/render_target",
            "/context/epoch",
        ] {
            let mut stale = ready.clone();
            *stale.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(
                !appkit_native_context_ready(&stale, &field, 32),
                "{pointer}"
            );
        }
    }

    #[test]
    fn stock_contract_requires_real_success_same_run_and_exact_cleanup() {
        let passed = json!({"status":"stock-control-ime-feasible", "child_exit_code":0,
            "process_exit_code":0, "native_bevy_validated":false,
            "cleanup":{"enabled_set_restored":true,"errors":[]},
            "final_field":{"committed":TEXT,"has_marked_text":false},
            "environment":{"run_id":"42"}});
        assert!(stock_success(&passed, "42"));
        assert!(!stock_success(&passed, "43"));
        let mut failed = passed.clone();
        failed["status"] = json!("passed");
        assert!(!stock_success(&failed, "42"));
        let mut dirty = passed;
        dirty["cleanup"]["enabled_set_restored"] = json!(false);
        assert!(!stock_success(&dirty, "42"));
    }

    #[test]
    fn focus_receipt_survives_new_inspection_ids_but_not_rebinding_or_focus_loss() {
        let inspected = |id: &str| {
            json!({"ui":{"focused_control":id,
            "surfaces":[{"controls":[{"id":id,"label":"Project name"}]}],
            "ime_diagnostics":{"overflow":false,"events":[],
                "focus":{"control_key":17,"binding":2,"label":"Project name",
                    "context":{"document_id":"owned","window_id":"main","epoch":3}}}}})
        };
        let first = inspected("control-1-2-3");
        let mut next = inspected("control-1-3-3");
        assert_eq!(focus(&first).unwrap(), focus(&next).unwrap());
        next["ui"]["ime_diagnostics"]["focus"]["binding"] = json!(3);
        assert_ne!(focus(&first).unwrap(), focus(&next).unwrap());
        next["ui"]["focused_control"] = Value::Null;
        assert!(focus(&next).is_err());
        let mut overflowed = first;
        overflowed["ui"]["ime_diagnostics"]["overflow"] = json!(true);
        assert!(focus(&overflowed).is_err());
    }
}
