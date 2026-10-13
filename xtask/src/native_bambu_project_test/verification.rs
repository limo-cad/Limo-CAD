//! Retained export controls exercise local verification without replacing an active slicer project.
use super::*;
use std::{path::Path, time::Instant};

fn poll(c: &mut Client, timeout: Duration) -> Result<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let result = bambu(c, "Refresh local verification", None)?;
        let report = result["value"].clone();
        let state = report["state"]
            .as_str()
            .context("Local verification report state")?;
        if !matches!(state, "queued" | "running") {
            return Ok(report);
        }
        ensure!(
            Instant::now() < deadline,
            "Retained local verification timed out: {report}"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn open(c: &mut Client, template: &Path, evidence: &Path) -> Result<()> {
    control(c, "File", None)?;
    control(c, "Export All Bodies as 3MF…", None)?;
    control(c, "3MF file mode", Some("bambu_project"))?;
    bambu(
        c,
        "Saved Bambu template path",
        Some(&template.to_string_lossy()),
    )?;
    bambu(c, "Inspect saved Bambu template", None)?;
    bambu(c, "Bambu placement source", Some("template"))?;
    bambu(
        c,
        "Saved Bambu target handoff",
        Some("Four-plate X2D fixture"),
    )?;
    bambu(c, "Keep template material and color", None)?;
    let preview = bambu(c, "Preview Bambu project", None)?;
    let bytes = fs::read(template)?;
    verify_template_z(
        &preview["value"]["report"],
        &limo_cad_export::bambu_project::read_bambu_volume_geometry(&bytes)?,
        evidence,
    )?;
    Ok(())
}

pub(super) fn run(c: &mut Client, out: &Path, artifact: &Path, body: &Value) -> Result<()> {
    let Some(executable) = std::env::var_os("LIMO_CAD_BAMBU_VERIFY_EXECUTABLE") else {
        return Ok(());
    };
    let executable = PathBuf::from(executable);
    ensure!(
        executable.is_absolute() && executable.is_file(),
        "Verification qualification requires an explicit installed local executable"
    );
    let original = model(c)?;
    let artifact_bytes = fs::read(artifact)?;
    let artifact_sha = crate::hash::hex(&Sha256::digest(&artifact_bytes));
    open(
        c,
        artifact,
        &out.join("local-verification-initial-preflight.json"),
    )?;
    let absent = out.join("deliberately-missing-BambuStudio.exe");
    ensure!(
        !absent.exists(),
        "Missing-slicer fixture path must actually be absent"
    );
    bambu(
        c,
        "Local Bambu Studio executable",
        Some(&absent.to_string_lossy()),
    )?;
    bambu(
        c,
        "Local verification timeout per plate (seconds)",
        Some("120"),
    )?;
    bambu(c, "Verify reviewed project locally", None)?;
    let missing = poll(c, Duration::from_secs(30))?;
    fs::write(
        out.join("local-verification-controls-missing-slicer.json"),
        serde_json::to_vec_pretty(&missing)?,
    )?;
    ensure!(
        missing["state"] == "failed",
        "Missing slicer may not claim verification: {missing}"
    );
    let missing_plates = missing["plates"]
        .as_array()
        .context("Missing-slicer plate results")?;
    ensure!(
        missing_plates
            .iter()
            .any(|p| p["state"] == "missing_slicer")
            && missing_plates
                .iter()
                .all(|p| p["toolpaths_generated"] == false),
        "Missing slicer must have actionable not-run evidence: {missing}"
    );

    bambu(
        c,
        "Local Bambu Studio executable",
        Some(&executable.to_string_lossy()),
    )?;
    let started = bambu(c, "Verify reviewed project locally", None)?;
    let completed = poll(c, Duration::from_secs(540))?;
    let native_result_path = out.join("local-verification-controls-native-result.json");
    fs::write(&native_result_path, serde_json::to_vec_pretty(&completed)?)?;
    ensure!(
        completed["state"] == "completed" && completed["stale"] == false,
        "Installed slicer qualification failed; inspect {}",
        native_result_path.display()
    );
    ensure!(
        completed["identity"]["project_sha256"] == artifact_sha,
        "Evidence must identify the exact reviewed artifact"
    );
    let plates = completed["plates"]
        .as_array()
        .context("Installed per-plate evidence")?;
    ensure!(
        plates.len() == 4
            && plates.iter().all(|p| p["exit_status"] == 0
                && p["toolpaths_generated"] == true
                && p["native_project_read_back"] == true
                && p["toolpath_sha256"].as_str().is_some_and(|s| s.len() == 64)
                && p["slicer_version"] == "02.08.02.61"),
        "Every actual plate must import, read back and slice: {completed}"
    );
    capture(c, out, "bambu-local-verification-completed")?;
    control(c, "Cancel", None)?;
    open(
        c,
        artifact,
        &out.join("local-verification-reopened-preflight.json"),
    )?;
    let reopened = bambu(c, "Refresh local verification", None)?;
    ensure!(
        reopened["value"]["job_id"] == completed["job_id"] && reopened["value"]["stale"] == false,
        "Closing export controls must retain usable exact-source evidence: {reopened}"
    );

    bambu(
        c,
        "Local Bambu Studio executable",
        Some(&executable.to_string_lossy()),
    )?;
    bambu(c, "Verify reviewed project locally", None)?;
    control(c, "Cancel", None)?;
    let before = c.call("cad_project_model", json!({}))?;
    c.call(
        "print_intent_set_part",
        json!({"body_id":body,"settings":{"wall_count":7},"expected_model_json":before}),
    )?;
    control(c, "File", None)?;
    control(c, "Export All Bodies as 3MF…", None)?;
    control(c, "3MF file mode", Some("bambu_project"))?;
    let cancelled = bambu(c, "Cancel local verification", None)?;
    ensure!(
        cancelled["value"]["cancel_requested"] == true,
        "Cancellation must remain available after the document revision changes: {cancelled}"
    );
    control(c, "Cancel", None)?;
    open(
        c,
        artifact,
        &out.join("local-verification-cancelled-preflight.json"),
    )?;
    bambu(c, "Local cancellation", None)?;
    capture(c, out, "bambu-local-cancellation-requested")?;
    let stopped = poll(c, Duration::from_secs(30))?;
    ensure!(
        stopped["state"] == "cancelled" && stopped["stale"] == true,
        "Owned child must stop and edited-source evidence must be stale: {stopped}"
    );
    fs::write(
        out.join("local-verification-controls-cancelled-result.json"),
        serde_json::to_vec_pretty(&stopped)?,
    )?;
    control(c, "Cancel", None)?;
    let after = c.call("cad_project_model", json!({}))?;
    c.call(
        "print_intent_set_document",
        json!({"document":original["print_intent"],"expected_model_json":after}),
    )?;
    open(
        c,
        artifact,
        &out.join("local-verification-restored-preflight.json"),
    )?;
    let restored = bambu(c, "Refresh local verification", None)?;
    ensure!(
        restored["value"]["stale"] == true,
        "Restoring previous settings must not revive invalidated evidence"
    );
    ensure!(
        model(c)? == original && fs::read(artifact)? == artifact_bytes,
        "Local verification must preserve the CAD project and reviewed artifact"
    );
    capture(c, out, "bambu-local-verification-cancelled-stale")?;
    control(c, "Cancel", None)?;
    fs::write(
        out.join("local-verification-controls-evidence.json"),
        serde_json::to_vec_pretty(
            &json!({"passed":true,"started":started,"missing_slicer":missing,"completed":completed,"reopened":reopened,"cancel_receipt":cancelled,"cancelled_stale":stopped,"restored_stale":restored,"source_and_artifact_unchanged":true,"physical_qualification":"not run"}),
        )?,
    )?;
    println!(
        "PASS actual Bevy local-verification controls: missing slicer, four real plates, modal reopening, cancellation after edit, sticky stale evidence"
    );
    Ok(())
}
