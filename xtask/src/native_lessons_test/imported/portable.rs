//! One owned native editor sequence for a portable replay, distinct from Preview.
use super::*;
use limo_cad_project_file::ReplayPackage;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;
const RESULT_NAME: &str = "Portable replay result";

struct Evidence {
    out: PathBuf,
    frames: Vec<Value>,
    build: Value,
}
impl Evidence {
    fn publish(&self, passed: bool) -> Result<()> {
        fs::write(
            self.out.join("frames.json"),
            serde_json::to_vec_pretty(&json!({"version":1,"state_checks_passed":passed,
                "capture":"Actual owned native window after control actions; not physical source typing",
                "desktop_build":self.build,"github_sha":std::env::var("GITHUB_SHA").ok(),
                "github_run_id":std::env::var("GITHUB_RUN_ID").ok(),
                "github_run_attempt":std::env::var("GITHUB_RUN_ATTEMPT").ok(),
                "frames":self.frames}))?,
        )?;
        let bytes = fs::read_dir(&self.out)?.try_fold(0_u64, |total, entry| {
            Ok::<_, std::io::Error>(total.saturating_add(entry?.metadata()?.len()))
        })?;
        ensure!(
            bytes <= MAX_EVIDENCE_BYTES,
            "Portable source evidence exceeds 64 MiB"
        );
        Ok(())
    }
    fn image(&mut self, name: &str, description: &str) -> Result<()> {
        let bytes = bounded_read(&self.out.join(name))?;
        ensure!(
            bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() > 1024,
            "Missing native image {name}"
        );
        self.frames.push(
            json!({"file":name,"sha256":digest(&bytes),"description":description,"delay_cs":100}),
        );
        self.publish(false)
    }
    fn checkpoint(&mut self, c: &mut Client, name: &str, description: &str) -> Result<Value> {
        let state = ui(c, json!({"action":"inspect"}))?;
        fs::write(
            self.out.join(format!("{name}.json")),
            serde_json::to_vec_pretty(&state)?,
        )?;
        capture(c, &self.out, name)?;
        self.image(&format!("{name}.png"), description)?;
        Ok(state)
    }
}
fn digest(bytes: &[u8]) -> String {
    crate::hash::hex(&Sha256::digest(bytes))
}
fn bounded_read(path: &Path) -> Result<Vec<u8>> {
    ensure!(
        fs::metadata(path)?.len() <= MAX_FILE_BYTES,
        "Fixture file exceeds 8 MiB: {}",
        path.display()
    );
    Ok(fs::read(path)?)
}
fn model(c: &mut Client) -> Result<Value> {
    serde_json::from_str(
        c.call("cad_project_model", json!({}))?
            .as_str()
            .context("Project model missing")?,
    )
    .context("Project model JSON")
}
fn status(state: &Value) -> &str {
    controls(state)
        .find(|row| row["label"] == "Script validation and save status")
        .and_then(|row| row["value"].as_str())
        .unwrap_or("")
}
fn run_enabled(state: &Value) -> bool {
    controls(state).any(|row| row["label"] == "Run in new design" && row["disabled"] == false)
}
fn wait(c: &mut Client, description: &str, ready: impl Fn(&Value) -> bool) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let state = ui(c, json!({"action":"inspect"}))?;
        if ready(&state) {
            return Ok(state);
        }
        ensure!(
            Instant::now() < deadline,
            "{description} did not finish: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
fn open(c: &mut Client, evidence: &mut Evidence, path: &Path) -> Result<Value> {
    let state = ui(c, json!({"action":"inspect"}))?;
    if controls(&state).any(|row| row["label"] == "Authored script source") {
        control(c, "Back to Scripts", None)?;
    }
    let state = ui(c, json!({"action":"inspect"}))?;
    if !controls(&state).any(|row| row["label"] == "Open script...") {
        control(c, "Scripts", None)?;
    }
    let started = control(c, "Open script...", None)?;
    ensure!(
        started["value"]["awaiting_input"] == true,
        "Script Open did not show a chooser: {started}"
    );
    let path_text = path.to_str().context("Portable source path Unicode")?;
    os_input::complete_dialog(c, &evidence.out, "Open Limo CAD script", path_text)?;
    evidence.image(
        &os_input::capture_name("Open Limo CAD script", path_text)?,
        "Open package in the owned OS chooser",
    )?;
    wait_source(
        c,
        path.file_name()
            .and_then(|s| s.to_str())
            .context("Package file name")?,
    )
}
fn save_draft(
    c: &mut Client,
    evidence: &mut Evidence,
    path: &Path,
    draft: &str,
    baseline: &[u8],
) -> Result<Value> {
    let started = control(c, "Save script as...", None)?;
    ensure!(
        started["value"]["awaiting_input"] == true,
        "Save script as did not show a chooser: {started}"
    );
    let path_text = path.to_str().context("Draft path Unicode")?;
    os_input::complete_dialog(c, &evidence.out, "Save Limo CAD script", path_text)?;
    evidence.image(
        &os_input::capture_name("Save Limo CAD script", path_text)?,
        "Save the unfinished package with the owned OS chooser",
    )?;
    let state = wait(c, "Package Save as", |state| {
        status(state).starts_with("Project replay saved with its retained baseline.")
            && controls(state).any(|row| {
                row["label"] == "Script source path and include directory"
                    && row["value"] == path_text
            })
    })?;
    let package = ReplayPackage::decode_cbor(&bounded_read(path)?)?;
    ensure!(
        package.script() == draft && package.project_bytes() == baseline,
        "Native Save as changed the draft or retained archive"
    );
    ensure!(!run_enabled(&state), "Saving an invalid draft enabled Run");
    Ok(state)
}

pub(super) fn exercise(
    c: &mut Client,
    root: &Path,
    original: &Value,
    original_session: &Value,
) -> Result<Value> {
    ensure!(os_input::enabled() && cfg!(target_os = "linux"),
        "Portable native acceptance requires the owned Linux Scripts launcher and OS chooser opt-in");
    let out = root.join("replay-source");
    fs::create_dir(&out).context("Choose a fresh portable source evidence directory")?;
    let original_model: Value =
        serde_json::from_str(original.as_str().context("Original model JSON")?)?;
    ensure!(
        model(c)? == original_model,
        "Portable fixture did not start on the retained original"
    );
    let observed = ui(c, json!({"action":"inspect"}))?;
    let build = observed["desktop_build"].clone();
    ensure!(
        build["revision"].is_string(),
        "Native build identity missing: {observed}"
    );
    if let Ok(expected) = std::env::var("GITHUB_SHA") {
        let revision = build["revision"].as_str().unwrap();
        ensure!(
            revision.len() >= 7 && expected.starts_with(revision) && build["modified"] == false,
            "Native build differs from this workflow checkout: {build}, expected {expected}"
        );
    }
    let mut evidence = Evidence {
        out,
        frames: Vec::new(),
        build,
    };
    evidence.publish(false)?;
    let baseline_path = evidence.out.join("baseline.limo");
    ui(
        c,
        json!({"action":"file","command":"save","path":baseline_path}),
    )?;
    let baseline = bounded_read(&baseline_path)?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&baseline))?;
    let archived_model: Value = serde_json::from_reader(archive.by_name("model.json")?)?;
    ensure!(
        archived_model == original_model,
        "Saved baseline differs from the original native model"
    );
    let source = serde_json::to_string_pretty(
        &json!({"version":2,"name":"Portable source acceptance",
        "starting_state":"project","steps":[
            {"chapter":"Retained baseline ready","note":"Inspect the original part before the source changes its name","duration_ms":1000},
            {"call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":RESULT_NAME}}},
            {"chapter":"Portable source complete","note":"The retained part now has the authored name","duration_ms":1}
        ]}),
    )?;
    let package_path = evidence.out.join("portable.limo.replay.cbor");
    ReplayPackage::new(source.clone(), baseline.clone())?.write_file_new(&package_path)?;
    let opened_bytes = bounded_read(&package_path)?;
    open(c, &mut evidence, &package_path)?;
    let opened = evidence.checkpoint(
        c,
        "01-opened",
        "Package source is ready; the retained design is unchanged",
    )?;
    ensure!(
        run_enabled(&opened) && model(c)? == original_model,
        "Opening a valid package changed the original or disabled Run"
    );

    let draft = "// Unfinished portable recovery draft\n{";
    control(c, "Authored script source", Some(draft))?;
    ensure!(
        !run_enabled(&ui(c, json!({"action":"inspect"}))?),
        "An unvalidated draft remained runnable"
    );
    control(c, "Validate source", None)?;
    wait(c, "Invalid source validation", |state| {
        status(state).starts_with("Script: ") && !run_enabled(state)
    })?;
    evidence.checkpoint(
        c,
        "02-invalid",
        "Invalid source reports a diagnostic and disables Run",
    )?;
    let recovery_path = evidence.out.join("unfinished.limo.replay.cbor");
    save_draft(c, &mut evidence, &recovery_path, draft, &baseline)?;
    evidence.checkpoint(
        c,
        "03-saved-draft",
        "Unfinished source and its exact baseline were saved",
    )?;
    open(c, &mut evidence, &recovery_path)?;
    let recovered = evidence.checkpoint(
        c,
        "04-recovered",
        "Reopened unfinished package remains editable and cannot Run",
    )?;
    ensure!(
        !run_enabled(&recovered)
            && !status(&recovered).is_empty()
            && controls(&recovered)
                .any(|row| row["label"] == "Authored script source" && row["value"] == draft),
        "Invalid package did not reopen as a diagnostic recovery draft: {recovered}"
    );
    let repaired = source.replace("Portable source acceptance", "Repaired portable source");
    control(c, "Authored script source", Some(&repaired))?;
    evidence.checkpoint(
        c,
        "05-repaired",
        "Repair the source without changing the retained part",
    )?;
    control(c, "Validate source", None)?;
    wait(c, "Repaired replay validation", |state| {
        status(state).starts_with("Replay source is valid.") && run_enabled(state)
    })?;
    evidence.checkpoint(
        c,
        "06-validated",
        "Validated edits are runnable but remain unsaved",
    )?;
    ensure!(
        model(c)? == original_model && bounded_read(&package_path)? == opened_bytes,
        "Package inspection, recovery or validation changed the original model/file"
    );
    ensure!(
        ReplayPackage::read_file(&recovery_path)?.script() == draft,
        "Repair or validation silently saved over the recovery draft"
    );

    let mode = control(c, "Script run mode", None)?;
    if mode["value"]["mode"] != "present" {
        control(c, "Script run mode", None)?;
    }
    let started = control(c, "Run in new design", None)?;
    ensure!(
        started["value"]["script_error"].is_null()
            && started["value"]["script_started"]["name"] == "Repaired portable source"
            && started["active_session_id"] != *original_session,
        "Prepared source did not start in a fresh retained tab: {started}"
    );
    let run_session = started["active_session_id"]
        .as_str()
        .context("Replay session missing")?
        .to_owned();
    let window = started["control_admission"]["window_id"]
        .as_str()
        .context("Run window identity missing")?;
    let process = started["control_admission"]["process_instance_id"]
        .as_str()
        .context("Run process identity missing")?;
    let document = started["value"]["document_id"]
        .as_str()
        .context("Run document identity missing")?;
    ensure!(
        started["control_admission"]["window_id"] == observed["control_admission"]["window_id"]
            && started["control_admission"]["process_instance_id"]
                == observed["control_admission"]["process_instance_id"]
            && started["value"]["document_id"] != observed["control_admission"]["document_id"],
        "Run did not retain the exact owned window/process and create a fresh document"
    );
    let run_receipt = serde_json::to_vec_pretty(&started)?;
    ensure!(
        run_receipt.len() as u64 <= MAX_FILE_BYTES,
        "Run receipt exceeds 8 MiB"
    );
    fs::write(evidence.out.join("run-started.json"), run_receipt)?;
    evidence.publish(false)?;
    let target = json!({"window_id":window,"document_id":document});
    let deadline = Instant::now() + Duration::from_secs(90);
    let successor = loop {
        // Baseline adoption retires the initial Run transport. Read the owned
        // registry without sending a control request through that stale route.
        let registry = c.call("cad_list_sessions", json!({}))?;
        let windows: Vec<_> = registry["windows"]
            .as_array()
            .context("Session windows missing")?
            .iter()
            .filter(|row| row["window_id"] == window && row["process_instance_id"] == process)
            .collect();
        ensure!(
            windows.len() <= 1,
            "Owned replay window is ambiguous: {registry}"
        );
        if let Some(row) = windows.first() {
            if let Some(active_document) = row["active_document_id"].as_str() {
                ensure!(
                    active_document == document,
                    "Replay active document changed: {row}"
                );
                if row["active_session_id"]
                    .as_str()
                    .is_some_and(|id| id != run_session)
                {
                    break (**row).clone();
                }
            }
        }
        ensure!(
            Instant::now() < deadline,
            "Owned replay successor was not published: {registry}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let attached = c.call("cad_attach", target.clone())?;
    ensure!(
        attached["window_id"] == window
            && attached["document_id"] == document
            && attached["session_id"] == successor["active_session_id"],
        "Replay attachment differs from its exact published successor: {attached}"
    );
    let completed = loop {
        let response = ui(c, json!({"action":"presentation","command":"status"}))?;
        let state = &response["presentation"];
        ensure!(state["stopped"] != true, "Portable source stopped: {state}");
        if state["finished"] == true && state["chapter"] == "Portable source complete" {
            break response;
        }
        ensure!(
            Instant::now() < deadline,
            "Portable source did not finish: {state}"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    let final_attachment = c.call("cad_attach", target.clone())?;
    ensure!(
        final_attachment["window_id"] == window
            && final_attachment["document_id"] == document
            && final_attachment["session_id"] == successor["active_session_id"],
        "Completed replay no longer belongs to its owned successor: {final_attachment}"
    );
    let mut expected = original_model.clone();
    expected["document"]["name"] = json!(RESULT_NAME);
    ensure!(
        model(c)? == expected,
        "Replay changed more than the authored document name"
    );
    let scene = c.call("solid_scene", json!({}))?;
    ensure!(
        scene["errors"].as_array().is_some_and(Vec::is_empty)
            && scene["bodies"]
                .as_array()
                .is_some_and(|rows| rows.len() == 1),
        "Replayed baseline lost its real solid: {scene}"
    );
    evidence.checkpoint(
        c,
        "07-run-complete",
        "Authored source completed in a new design; original part retained",
    )?;
    let result_path = evidence.out.join("portable-result.limo");
    let saved = ui(
        c,
        json!({"action":"file","command":"save","path":result_path}),
    )?;
    evidence.checkpoint(c, "08-saved-result", "Save the resulting editable project")?;
    let reopened = ui(
        c,
        json!({"action":"file","command":"open","path":result_path}),
    )?;
    ensure!(
        reopened["value"]["result"]["opened"] == true,
        "Native Open did not read the saved result: {reopened}"
    );
    ensure!(
        reopened["control_admission"]["window_id"] == window
            && reopened["control_admission"]["process_instance_id"] == process
            && reopened["value"]["document_id"] == document,
        "Result Open changed the owned replay target: {reopened}"
    );
    // Resolve the published Open successor before any inspection through the
    // previous transport. Ordinary Open may retire that transport too.
    let reopened_attachment = c.call("cad_attach", target.clone())?;
    ensure!(
        reopened_attachment["window_id"] == window
            && reopened_attachment["document_id"] == document
            && reopened_attachment["session_id"] == reopened["active_session_id"],
        "Reopened result attachment differs from its native completion: {reopened_attachment}"
    );
    ensure!(
        model(c)? == expected,
        "Reopening the replayed project lost retained intent or the source result"
    );
    evidence.checkpoint(
        c,
        "09-reopened-result",
        "Reopened saved result matches the complete expected model",
    )?;
    let restored = control(c, "Retained lesson", None)?;
    ensure!(
        restored["active_session_id"] == *original_session,
        "Original tab identity changed"
    );
    c.call("cad_attach", json!({"session_id":original_session}))?;
    ensure!(
        model(c)? == original_model
            && bounded_read(&baseline_path)? == baseline
            && bounded_read(&package_path)? == opened_bytes,
        "Replay or result reopening changed the original tab, archive or source package"
    );
    let recovery = ReplayPackage::decode_cbor(&bounded_read(&recovery_path)?)?;
    ensure!(
        recovery.script() == draft && recovery.project_bytes() == baseline,
        "Recovery package source or retained baseline changed during Run"
    );
    evidence.checkpoint(
        c,
        "10-retained-original",
        "Original native tab and saved archive remain unchanged",
    )?;
    let report = json!({"state_checks_passed":true,"pixel_review":"required",
        "desktop_build":evidence.build,"original_session":original_session,"initial_run_session":run_session,
        "replay_target":target,"successor_attachment":attached,"final_attachment":final_attachment,
        "baseline_sha256":digest(&baseline),"opened_package_sha256":digest(&opened_bytes),
        "recovery_package_sha256":digest(&bounded_read(&recovery_path)?),
        "original_model":original_model,"expected_result_model":expected,
        "started":started,"completed":completed,
        "scene":scene,"saved_result":saved,"reopened_result":reopened,"reopened_attachment":reopened_attachment,
        "checks":["native-package-open-preserves-original","invalid-draft-disables-run",
            "real-os-source-save-chooser","exact-retained-archive-bytes","invalid-draft-recovery",
            "repair-validation-does-not-save","complete-model-retained-through-normal-replay",
            "fresh-retained-tab","name-only-complete-model-oracle","real-solid-retained",
            "saved-result-reopens","original-tab-and-files-unchanged"],
        "not_proven":["Separate Prepare UI: preparation is internal to Run",
            "Physical source typing, keyboard entry into Bevy, IME or pointer activation",
            "Project Save/Open OS choosers: project paths use the production native file command",
            "Manual pixel review, non-Linux choosers or broad UI/MCP qualification"]});
    fs::write(
        evidence.out.join("acceptance.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    evidence.publish(true)?;
    Ok(json!({"state_checks_passed":true,"pixel_review":"required",
        "evidence":"replay-source/acceptance.json","frames":"replay-source/frames.json",
        "operational_gif":"replay-source/portable-source.gif","gif_created_by":"scoped scripts-family CI step"}))
}
