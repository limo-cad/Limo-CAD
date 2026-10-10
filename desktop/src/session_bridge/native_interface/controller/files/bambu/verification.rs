//! Optional local slicing uses the shared writer and keeps evidence outside the export draft.
use super::*;
use limo_cad_export::slicer_verification::{
    LocalSlicerOptions, LocalSlicerReport, LocalSlicerStartRequest, VerificationState,
};

#[derive(Clone, Debug)]
struct Job {
    owner: DocumentContext,
    revision: u64,
    request_key: Value,
    report: LocalSlicerReport,
    cancel_requested: bool,
}

#[derive(Default)]
pub(in super::super) struct Jobs(Vec<Job>);

fn same_tab(a: &DocumentContext, b: &DocumentContext) -> bool {
    a.window_id == b.window_id && a.document_id == b.document_id
}
fn active(report: &LocalSlicerReport) -> bool {
    matches!(
        report.state,
        VerificationState::Queued | VerificationState::Running
    )
}
fn job(world: &World) -> Option<&Job> {
    let files = world.resource::<Files>();
    let owner = &files.dialog.as_ref()?.receipt.owner;
    owned_job(world, owner)
}
fn owned_job<'a>(world: &'a World, owner: &DocumentContext) -> Option<&'a Job> {
    let files = world.resource::<Files>();
    files
        .verification
        .0
        .iter()
        .find(|j| same_tab(&j.owner, owner))
}
pub(super) fn has_job(world: &World) -> bool {
    job(world).is_some()
}
fn key(intent: &io::ExportIntent) -> Result<Value, String> {
    Ok(review_key(
        &request_with_template(intent, false)?,
        &intent.bambu.template()?.summary,
    ))
}
fn options(intent: &io::ExportIntent) -> Result<LocalSlicerOptions, String> {
    let timeout = field_text(intent, Field::VerifierTimeout)
        .parse::<u32>()
        .map_err(|_| "Enter a whole-number per-plate timeout between 1 and 600 seconds")?;
    if !(1..=600).contains(&timeout) {
        return Err("Per-plate timeout must be between 1 and 600 seconds".into());
    }
    let executable = PathBuf::from(&intent.bambu.verifier_path);
    if !executable.is_absolute() {
        return Err("Choose an absolute local Bambu Studio executable path".into());
    }
    Ok(LocalSlicerOptions {
        executable,
        timeout_seconds_per_plate: timeout,
    })
}
fn start_request(intent: &io::ExportIntent) -> Result<LocalSlicerStartRequest, String> {
    if !intent.bambu.enabled {
        return Err("Choose explicit Bambu project mode before local verification".into());
    }
    check_review(intent)?;
    io::check_layout_confirmation(intent)?;
    Ok(LocalSlicerStartRequest {
        options: options(intent)?,
        project: request(intent)?,
    })
}
pub(super) fn can_start(world: &World, intent: &io::ExportIntent) -> Result<(), String> {
    if !intent.bambu.enabled {
        return Err("Choose Bambu project mode".into());
    }
    check_review(intent)?;
    io::check_layout_confirmation(intent)?;
    options(intent)?;
    if job(world).is_some_and(|j| active(&j.report) && !j.cancel_requested) {
        return Err(
            "Refresh or cancel the current local verification before starting another".into(),
        );
    }
    let jobs = &world.resource::<Files>().verification.0;
    if jobs.len() >= 16
        && jobs
            .iter()
            .all(|j| active(&j.report) && !j.cancel_requested)
    {
        return Err("Refresh or cancel earlier local verification jobs first".into());
    }
    Ok(())
}

fn save(
    world: &mut World,
    owner: DocumentContext,
    revision: u64,
    request_key: Value,
    report: LocalSlicerReport,
) {
    let jobs = &mut world.resource_mut::<Files>().verification.0;
    let cancel_requested = jobs.iter().any(|j| {
        same_tab(&j.owner, &owner) && j.report.job_id == report.job_id && j.cancel_requested
    });
    jobs.retain(|j| !same_tab(&j.owner, &owner));
    if jobs.len() >= 16 {
        if let Some(index) = jobs
            .iter()
            .position(|j| !active(&j.report) || j.cancel_requested)
        {
            jobs.remove(index);
        }
    }
    jobs.push(Job {
        owner,
        revision,
        request_key,
        report,
        cancel_requested,
    });
}

pub(super) fn reduce(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    token: u64,
    command: Command,
) -> Result<Value, String> {
    let mut dialog = if command == Command::VerifyStart {
        owned_dialog(world, services, owner, token)?
    } else {
        let dialog = world
            .resource::<Files>()
            .dialog
            .clone()
            .ok_or("Open the 3MF export controls")?;
        if dialog.token != token || &dialog.receipt.owner != owner {
            return Err("Export dialog was replaced".into());
        }
        dialog
    };
    dialog.receipt = services
        .bridge
        .native_document_receipt(&services.engine, owner)?;
    let DialogKind::Export(intent) = dialog.kind else {
        return Err("Open the 3MF export controls".into());
    };
    let (operation, arguments, request_key) = if command == Command::VerifyStart {
        can_start(world, &intent)?;
        (
            "bambu_local_verification_start",
            serde_json::to_value(start_request(&intent)?).map_err(|e| e.to_string())?,
            key(&intent)?,
        )
    } else {
        let job = job(world).ok_or("This tab has no local verification job")?;
        let operation = if command == Command::VerifyCancel {
            "bambu_local_verification_cancel"
        } else {
            "bambu_local_verification_poll"
        };
        (
            operation,
            json!({"job_id":job.report.job_id}),
            job.request_key.clone(),
        )
    };
    let callback_owner = owner.clone();
    worker::enqueue_query(
        world,
        owner.clone(),
        dialog.receipt.revision,
        operation.into(),
        arguments,
        move |world, services, result| {
            let result = result?;
            if command == Command::VerifyCancel {
                if result.value["cancel_requested"] == true {
                    if let Some(job) = world
                        .resource_mut::<Files>()
                        .verification
                        .0
                        .iter_mut()
                        .find(|j| {
                            same_tab(&j.owner, &callback_owner)
                                && Some(j.report.job_id) == result.value["job_id"].as_u64()
                        })
                    {
                        job.cancel_requested = true;
                    }
                }
                return Ok(result.value);
            }
            let mut report: LocalSlicerReport =
                serde_json::from_value(result.value).map_err(|e| e.to_string())?;
            let old_stale = owned_job(world, &callback_owner)
                .is_some_and(|j| j.report.job_id == report.job_id && j.report.stale);
            report.stale |= old_stale;
            let current = services
                .bridge
                .native_document_receipt(&services.engine, &callback_owner);
            report.stale |= current
                .as_ref()
                .map_or(true, |r| r.revision != result.engine_revision);
            if let Some(dialog) = &world.resource::<Files>().dialog {
                if dialog.receipt.owner == callback_owner {
                    if let DialogKind::Export(intent) = &dialog.kind {
                        report.stale |= check_review(intent).is_ok()
                            && key(intent).map_or(true, |current| current != request_key);
                    }
                }
            }
            let value = serde_json::to_value(&report).map_err(|e| e.to_string())?;
            save(
                world,
                callback_owner.clone(),
                result.engine_revision,
                request_key,
                report,
            );
            Ok(value)
        },
    )
}

pub(in super::super) fn observe(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
) {
    let Some(dialog) = world.resource::<Files>().dialog.as_ref() else {
        return;
    };
    let DialogKind::Export(intent) = &dialog.kind else {
        return;
    };
    if !intent.bambu.enabled {
        return;
    }
    let current_key = key(intent);
    let reviewed = check_review(intent).is_ok();
    let revision = services
        .bridge
        .native_document_receipt(&services.engine, owner)
        .map(|r| r.revision);
    if let Some(job) = world
        .resource_mut::<Files>()
        .verification
        .0
        .iter_mut()
        .find(|j| same_tab(&j.owner, owner))
    {
        job.report.stale |= &job.owner != owner
            || revision.as_ref().map_or(true, |r| *r != job.revision)
            || (reviewed && (current_key.as_ref() != Ok(&job.request_key)));
    }
}

use super::panel::{information, Row};
pub(super) fn rows(world: &World, intent: &io::ExportIntent, rows: &mut Vec<Row>) {
    information(
        rows,
        "Local slicer verification",
        "Optional, local temporary copies only. Export works without it. No print/send commands; physical fit and strength remain unqualified.",
    );
    rows.extend([
        (
            "Local Bambu Studio executable".into(),
            Some(Field::VerifierPath),
            None,
            None,
        ),
        (
            "Local verification timeout per plate (seconds)".into(),
            Some(Field::VerifierTimeout),
            None,
            None,
        ),
        (
            "Verify reviewed project locally".into(),
            None,
            Some(Command::VerifyStart),
            None,
        ),
        (
            "Refresh local verification".into(),
            None,
            Some(Command::VerifyPoll),
            None,
        ),
        (
            "Cancel local verification".into(),
            None,
            Some(Command::VerifyCancel),
            None,
        ),
    ]);
    if let Err(error) = can_start(world, intent) {
        information(rows, "Local verification precondition", error);
    }
    let Some(job) = job(world) else {
        information(
            rows,
            "Local verification evidence",
            "Not run. Metadata readback alone does not establish slicer import or generated toolpaths.",
        );
        return;
    };
    let source = intent
        .bambu
        .document
        .as_ref()
        .and_then(|d| d.source_document_id.as_deref());
    if source != Some(job.report.identity.source_document_id.as_str()) {
        information(
            rows,
            "Previous owned verification",
            format!(
                "Job {} belongs to a previous source document. Cancellation requested: {}. Its report is not disclosed in this project.",
                job.report.job_id, job.cancel_requested
            ),
        );
        return;
    }
    let owner = &world
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .receipt
        .owner;
    let stale = job.report.stale
        || &job.owner != owner
        || key(intent).map_or(true, |current| current != job.request_key);
    information(
        rows,
        "Local verification state",
        format!(
            "Job {}: {:?}; evidence {}. Refresh to check current geometry/layout. Physical checks: {}.",
            job.report.job_id,
            job.report.state,
            if stale {
                "stale"
            } else {
                "captured for this reviewed source"
            },
            job.report.physical_qualification
        ),
    );
    if job.cancel_requested {
        information(
            rows,
            "Local cancellation",
            "Cancellation requested. Refresh local verification to see the final child and per-plate results.",
        );
    }
    information(
        rows,
        "Verified local executable",
        job.report.executable.to_string_lossy(),
    );
    information(
        rows,
        "Verification artifact SHA256",
        &job.report.identity.project_sha256,
    );
    information(
        rows,
        "Verification model SHA256",
        &job.report.identity.source_model_sha256,
    );
    information(
        rows,
        "Verification print intent SHA256",
        &job.report.identity.print_intent_sha256,
    );
    information(
        rows,
        "Verification target placement SHA256",
        &job.report.identity.resolved_layout_sha256,
    );
    information(
        rows,
        "Verification profile SHA256",
        &job.report.identity.profile_sha256,
    );
    for plate in &job.report.plates {
        let label = format!("Verified plate {}", plate.plate);
        information(
            rows,
            &label,
            format!(
                "{:?}; exit {:?}; {} ms; slicer {:?}; native geometry/settings readback {}; generated toolpaths {}.",
                plate.state,
                plate.exit_status,
                plate.elapsed_milliseconds,
                plate.slicer_version,
                plate.native_project_read_back,
                plate.toolpaths_generated
            ),
        );
        if let Some(hash) = &plate.toolpath_sha256 {
            information(rows, format!("{label} G-code SHA256"), hash);
        }
        if let Some(message) = &plate.message {
            information(rows, format!("{label} result"), message);
        }
        for (index, issue) in plate
            .compatibility_issues
            .iter()
            .chain(&plate.setting_changes)
            .enumerate()
        {
            information(rows, format!("{label} issue {}", index + 1), issue);
        }
        if let Some(values) = &plate.native_effective_settings {
            if let Some(effective) = values["effective"].as_object() {
                for (identity, value) in effective
                    .iter()
                    .filter(|(key, _)| key.starts_with("volume:"))
                {
                    information(
                        rows,
                        format!("{label} native {identity}"),
                        value["settings"].to_string(),
                    );
                }
            }
        }
        if let Some(native) = &plate.native_result {
            if let Some(seconds) = native["total_predication"].as_f64() {
                information(
                    rows,
                    format!("{label} slicer time estimate"),
                    format!("{seconds:.0} seconds; estimate from native slicing"),
                );
            }
            if let Some(filaments) = native["filaments"].as_array() {
                for filament in filaments {
                    if let Some(grams) = filament["total_used_g"].as_f64() {
                        information(
                            rows,
                            format!("{label} filament {} estimate", filament["id"]),
                            format!(
                                "{grams:.2} g; logical profile slot, no physical tray assignment"
                            ),
                        );
                    }
                }
            }
            if let Some(warning) = native["warning_message"].as_str().filter(|s| !s.is_empty()) {
                information(rows, format!("{label} native warning"), warning);
            }
        }
    }
    for (index, warning) in job.report.warnings.iter().enumerate() {
        information(rows, format!("Verification note {}", index + 1), warning);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_export::slicer_verification::VerificationIdentity;

    fn report(source: &str) -> LocalSlicerReport {
        LocalSlicerReport {
            job_id: 7,
            state: VerificationState::Completed,
            identity: VerificationIdentity {
                source_document_id: source.into(),
                project_sha256: "private-artifact-hash".into(),
                source_model_sha256: "model-hash".into(),
                source_geometry_revision: None,
                print_intent_sha256: "intent-hash".into(),
                source_layout_sha256: "source-layout-hash".into(),
                resolved_layout_sha256: "target-layout-hash".into(),
                named_view: None,
                profile_sha256: "profile-hash".into(),
            },
            executable: PathBuf::from("D:/fixture/BambuStudio.exe"),
            executable_sha256: None,
            plates: vec![],
            stale: false,
            physical_qualification: "not run".into(),
            warnings: vec![],
        }
    }

    #[test]
    fn evidence_survives_closed_controls_and_stays_stale_after_document_replacement() {
        use super::super::super::tests::setup;
        use crate::session_bridge::native_interface::tests::Fixture;
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let (mut app, services, _) = setup(&fixture);
        let owner = fixture.owner();
        let receipt = current(app.world(), &services, &owner).unwrap();
        let intent = super::super::tests::intent();
        let source = intent
            .bambu
            .document
            .as_ref()
            .unwrap()
            .source_document_id
            .as_deref()
            .unwrap();
        let dialog = Dialog {
            token: 17,
            receipt: receipt.clone(),
            kind: DialogKind::Export(Arc::new(intent.clone())),
            error: None,
        };
        app.world_mut().resource_mut::<Files>().dialog = Some(dialog.clone());
        save(
            app.world_mut(),
            owner.clone(),
            receipt.revision,
            key(&intent).unwrap(),
            report(source),
        );
        app.world_mut().resource_mut::<Files>().dialog = None;
        assert!(job(app.world()).is_none());
        assert!(!owned_job(app.world(), &owner).unwrap().report.stale);
        app.world_mut().resource_mut::<Files>().dialog = Some(dialog.clone());
        observe(app.world_mut(), &services, &owner);
        assert!(!job(app.world()).unwrap().report.stale);

        {
            let mut files = app.world_mut().resource_mut::<Files>();
            files.dialog.as_mut().unwrap().receipt.owner.epoch += 1;
        }
        let mut replaced = owner.clone();
        replaced.epoch += 1;
        observe(app.world_mut(), &services, &replaced);
        assert!(job(app.world()).unwrap().report.stale);
        app.world_mut().resource_mut::<Files>().dialog = Some(dialog);
        observe(app.world_mut(), &services, &owner);
        assert!(
            job(app.world()).unwrap().report.stale,
            "Returning to a previous document incarnation must not revive invalidated evidence"
        );
    }

    #[test]
    fn previous_source_evidence_is_hidden_and_another_tab_cannot_retrieve_it() {
        let owner = DocumentContext {
            window_id: "main".into(),
            document_id: "one".into(),
            epoch: 1,
        };
        let mut intent = super::super::tests::intent();
        let old_source = intent
            .bambu
            .document
            .as_ref()
            .unwrap()
            .source_document_id
            .clone()
            .unwrap();
        intent.bambu.document.as_mut().unwrap().source_document_id = Some("replaced-source".into());
        let mut world = World::new();
        world.insert_resource(Files::default());
        world.resource_mut::<Files>().dialog = Some(Dialog {
            token: 17,
            receipt: super::super::super::DocumentReceipt {
                owner: owner.clone(),
                revision: 1,
            },
            kind: DialogKind::Export(Arc::new(intent.clone())),
            error: None,
        });
        save(
            &mut world,
            owner.clone(),
            1,
            key(&intent).unwrap(),
            report(&old_source),
        );
        let mut values = vec![];
        rows(&world, &intent, &mut values);
        assert!(values
            .iter()
            .any(|(label, _, _, _)| label == "Previous owned verification"));
        assert!(!values.iter().any(|(_, _, _, value)| {
            value
                .as_deref()
                .is_some_and(|v| v.contains("private-artifact-hash"))
        }));
        let mut other = owner;
        other.document_id = "other".into();
        world
            .resource_mut::<Files>()
            .dialog
            .as_mut()
            .unwrap()
            .receipt
            .owner = other;
        assert!(job(&world).is_none());
    }

    #[test]
    fn slicer_options_require_explicit_local_path_and_bounded_timeout() {
        let mut intent = super::super::tests::intent();
        intent.bambu.verifier_path = "relative.exe".into();
        intent.bambu.verifier_timeout = "NaN".into();
        assert!(options(&intent).unwrap_err().contains("whole-number"));
        intent.bambu.verifier_timeout = "601".into();
        assert!(options(&intent).unwrap_err().contains("between"));
        intent.bambu.verifier_timeout = "120".into();
        assert!(options(&intent).unwrap_err().contains("absolute"));
        intent.bambu.verifier_timeout = "120".into();
        intent.bambu.verifier_path = std::env::current_exe().unwrap().to_string_lossy().into();
        assert_eq!(options(&intent).unwrap().timeout_seconds_per_plate, 120);
        assert!(start_request(&intent).unwrap_err().contains("Preview"));
    }
}
