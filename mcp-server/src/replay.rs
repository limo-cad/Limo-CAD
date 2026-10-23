//! Shared read-only preparation for native and MCP project replay. No engine,
//! session attachment, publication, playback configuration or mutation occurs.
use limo_cad_project_file::{ProjectArchive, ReplayPackage, ReplayPackageIdentity};
use limo_cad_script::{ProjectScript, RunOptions};
use std::path::Path;

/// Retains the original archive (including ancillary entries), parsed v2 script
/// and validated options. Preparation grants no document or execution authority.
pub struct PreparedProjectReplay {
    script: ProjectScript,
    archive: ProjectArchive,
    options: RunOptions,
    identity: ReplayPackageIdentity,
}
impl PreparedProjectReplay {
    pub fn script(&self) -> &ProjectScript {
        &self.script
    }
    pub fn identity(&self) -> &ReplayPackageIdentity {
        &self.identity
    }
    /// The host must stage domain/kernel validation, admit the original owner,
    /// adopt the archive, and acknowledge the exact successor before any steps.
    /// Retain these values across acknowledgement retries; never reread the path.
    pub fn into_parts(
        self,
    ) -> (
        ProjectScript,
        ProjectArchive,
        RunOptions,
        ReplayPackageIdentity,
    ) {
        (self.script, self.archive, self.options, self.identity)
    }
}

pub fn prepare_project_replay(
    path: &Path,
    expected: &ReplayPackageIdentity,
    presentation: bool,
    validate: bool,
) -> Result<PreparedProjectReplay, String> {
    let package = ReplayPackage::read_file_verified(path, expected).map_err(|e| e.to_string())?;
    prepare(
        package,
        expected.clone(),
        RunOptions {
            presentation,
            validate,
        },
    )
}

/// Inspect only edited v2 source, catalog calls, options and chapter ranges.
/// Archive/domain validity and document authority are deliberately not implied.
/// Package source is self-contained; unresolved includes are rejected.
pub fn inspect_project_replay_source(
    source: &str,
    presentation: bool,
    validate: bool,
) -> Result<serde_json::Value, String> {
    let script = validated_source(
        source,
        RunOptions {
            presentation,
            validate,
        },
    )?;
    let mut result = script.script().metadata();
    result["authored_chapters"] = limo_cad_script::authored_chapters(
        source,
        result["step_count"].as_u64().unwrap_or(0) as usize,
    )?;
    result["source"] = source.into();
    result["authored_source"] = source.into();
    result["project_baseline_required"] = true.into();
    Ok(result)
}

/// Prepare an immutable editor snapshot with its retained package, without any
/// file read. Borrows the editor package so both failed and successful preparation
/// leave its baseline available for subsequent edits and saves. After source
/// preflight, one bounded archive copy is decoded for the owned run. Callers must
/// serialize preparation jobs; do not prepare on every keystroke or validation.
/// Identity describes the edited canonical envelope, not the original selection
/// and not proof of a successful save.
pub fn prepare_project_replay_source(
    package: &ReplayPackage,
    source: &str,
    presentation: bool,
    validate: bool,
) -> Result<PreparedProjectReplay, String> {
    let options = RunOptions {
        presentation,
        validate,
    };
    let script = validated_source(source, options)?;
    let identity = package.source_identity(source).map_err(|e| e.to_string())?;
    let archive =
        ProjectArchive::decode(package.project_bytes().to_vec()).map_err(|e| e.to_string())?;
    finish_archive(archive, script, identity, options)
}

fn validated_source(source: &str, options: RunOptions) -> Result<ProjectScript, String> {
    let script = ProjectScript::parse(source)?;
    script.script().validate_options(options)?;
    crate::interface::validate_script(script.script())?;
    Ok(script)
}

fn prepare(
    package: ReplayPackage,
    identity: ReplayPackageIdentity,
    options: RunOptions,
) -> Result<PreparedProjectReplay, String> {
    // Validate every declared call/check before archive decoding or live work.
    let script = validated_source(package.script(), options)?;
    finish_preparation(package, script, identity, options)
}

fn finish_preparation(
    package: ReplayPackage,
    script: ProjectScript,
    identity: ReplayPackageIdentity,
    options: RunOptions,
) -> Result<PreparedProjectReplay, String> {
    let (_, archive) = package.into_project().map_err(|e| e.to_string())?;
    finish_archive(archive, script, identity, options)
}

fn finish_archive(
    archive: ProjectArchive,
    script: ProjectScript,
    identity: ReplayPackageIdentity,
    options: RunOptions,
) -> Result<PreparedProjectReplay, String> {
    // Validate snapshot framing/limits while retaining the full archive. Typed
    // domain semantics and kernel staging remain the guarded host's responsibility.
    archive.snapshot().map_err(|e| e.to_string())?;
    Ok(PreparedProjectReplay {
        script,
        archive,
        options,
        identity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_project_file::SaveMetadata;
    use serde_json::json;
    fn source(version: u32, group: &str) -> String {
        json!({"version":version,"name":"Retained project","starting_state":"project",
            "steps":[{"call":{"group":group,"operation":"cad_set_document_name","arguments":{"name":"After"}}}]}).to_string()
    }
    fn archive() -> Vec<u8> {
        let mut archive = ProjectArchive::new(
            r#"{"format":"limo-cad-project","schema_version":15}"#.into(),
            SaveMetadata {
                application_version: "test",
                saved_at: "2026-10-17T00:00:00Z",
            },
        )
        .unwrap();
        archive
            .set_intent("intent/future.cbor", Some(vec![0, 255, 7]))
            .unwrap();
        archive.encode().unwrap()
    }
    #[test]
    fn edited_preparation_uses_current_source_and_retains_original_archive() {
        let original = archive();
        let package = ReplayPackage::new(source(2, "document/files"), original.clone()).unwrap();
        let old_identity = package.source_identity(package.script()).unwrap();
        let edited = source(2, "document/files").replace("Retained project", "Edited project");
        let expected = package.source_identity(&edited).unwrap();
        assert_ne!(old_identity, expected);
        let inspected = inspect_project_replay_source(&edited, false, true).unwrap();
        assert_eq!(inspected["name"], "Edited project");
        assert_eq!(inspected["authored_source"], edited);
        assert_eq!(inspected["project_baseline_required"], true);
        let prepared = prepare_project_replay_source(&package, &edited, false, true).unwrap();
        assert_eq!(prepared.identity(), &expected);
        assert_eq!(
            prepared.script().script().metadata()["name"],
            "Edited project"
        );
        let (_, retained, _, _) = prepared.into_parts();
        assert_eq!(retained.encode().unwrap(), original);
        assert_eq!(
            retained.snapshot().unwrap().intents()[0].payload,
            [0, 255, 7]
        );
    }

    #[test]
    fn editor_source_validation_matches_preparation_and_does_not_validate_archive() {
        for bad in [
            source(1, "document/files"),
            source(2, "wrong/group"),
            String::new(),
        ] {
            let inspection = inspect_project_replay_source(&bad, false, true).unwrap_err();
            let package = ReplayPackage::new("original".into(), vec![255]).unwrap();
            let preparation = prepare_project_replay_source(&package, &bad, false, true)
                .err()
                .unwrap();
            assert_eq!(inspection, preparation);
        }
        let valid = source(2, "document/files");
        assert!(inspect_project_replay_source(&valid, false, true).is_ok());
        let package = ReplayPackage::new("original".into(), vec![255]).unwrap();
        assert!(prepare_project_replay_source(&package, &valid, false, true).is_err());
    }
    #[test]
    fn editor_retains_baseline_after_failed_and_successful_preparation() {
        use std::sync::Arc;
        let original = archive();
        let package =
            Arc::new(ReplayPackage::new(source(2, "document/files"), original.clone()).unwrap());
        let editor = Arc::clone(&package);
        let before = editor.encode_cbor().unwrap();
        let address = editor.project_bytes().as_ptr();
        assert!(prepare_project_replay_source(&package, "invalid draft", false, true).is_err());
        assert_eq!(editor.project_bytes().as_ptr(), address);
        assert_eq!(editor.encode_cbor().unwrap(), before);
        let edited = source(2, "document/files").replace("Retained project", "First run");
        let prepared = prepare_project_replay_source(&package, &edited, false, true).unwrap();
        // The editor can continue to encode another draft while this owned run
        // retains the old generation. Neither source nor archive is taken away.
        let later = edited.replace("First run", "Later draft");
        let later_package =
            ReplayPackage::decode_cbor(&editor.encode_source_cbor(&later).unwrap()).unwrap();
        assert_eq!(later_package.script(), later);
        assert_eq!(later_package.project_bytes(), original);
        assert_eq!(prepared.script().script().metadata()["name"], "First run");
        drop(prepared);
        assert_eq!(editor.project_bytes().as_ptr(), address);
        assert_eq!(editor.encode_cbor().unwrap(), before);
        let malformed = ReplayPackage::new("retained original".into(), vec![255]).unwrap();
        assert!(prepare_project_replay_source(&malformed, &edited, false, true).is_err());
        assert_eq!(malformed.script(), "retained original");
        assert_eq!(malformed.project_bytes(), [255]);
    }

    #[test]
    fn edited_chapter_ranges_and_required_options_are_preserved() {
        let source = r#"// authored comment
{"version":2,"name":"Chapter","starting_state":"project","steps":[{"note":"Inspect baseline"}]}"#;
        let inspected = inspect_project_replay_source(source, false, true).unwrap();
        let range = &inspected["authored_chapters"][0];
        let start = range["start"].as_u64().unwrap() as usize;
        let end = range["end"].as_u64().unwrap() as usize;
        assert_eq!(&source[start..end], r#""Inspect baseline""#);
        let mut gated: serde_json::Value =
            serde_json::from_str(source.lines().nth(1).unwrap()).unwrap();
        gated["verification"] = json!("garden-bench");
        assert!(
            inspect_project_replay_source(&gated.to_string(), false, false)
                .unwrap_err()
                .contains("requires its final verification gate")
        );
        gated.as_object_mut().unwrap().remove("verification");
        gated["includes"] = json!(["external.limo.jsonc"]);
        assert!(inspect_project_replay_source(&gated.to_string(), false, true).is_err());
    }
    #[test]
    fn preparation_retains_original_archive_and_does_not_reopen_source() {
        let path = std::env::temp_dir().join(format!(
            "limo-prepared-replay-{}-{}.cbor",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let original = archive();
        let package = ReplayPackage::new(source(2, "document/files"), original.clone()).unwrap();
        package.write_file_new(&path).unwrap();
        let (_, identity) = ReplayPackage::read_file_with_identity(&path).unwrap();
        let prepared = prepare_project_replay(&path, &identity, false, true).unwrap();
        assert_eq!(prepared.identity(), &identity);
        std::fs::write(&path, b"changed after preparation").unwrap();
        assert!(prepare_project_replay(&path, &identity, false, true).is_err());
        let (_, retained, options, retained_identity) = prepared.into_parts();
        assert_eq!(retained.encode().unwrap(), original);
        assert_eq!(
            retained.snapshot().unwrap().intents()[0].payload,
            [0, 255, 7]
        );
        assert!(!options.presentation && options.validate);
        assert_eq!(retained_identity, identity);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn invalid_script_catalog_options_and_archive_fail_preparation() {
        let identity = ReplayPackageIdentity {
            byte_length: 1,
            sha256: [0; 32],
        };
        let options = RunOptions {
            presentation: false,
            validate: true,
        };
        for (source, bytes) in [
            (source(1, "document/files"), archive()),
            (source(2, "wrong/group"), archive()),
            (source(2, "document/files"), vec![0xff]),
        ] {
            let package = ReplayPackage::new(source, bytes).unwrap();
            assert!(prepare(package, identity.clone(), options).is_err());
        }
        let mut gated: serde_json::Value =
            serde_json::from_str(&source(2, "document/files")).unwrap();
        gated["verification"] = json!("garden-bench");
        let gated = ReplayPackage::new(gated.to_string(), vec![0xff]).unwrap();
        let error = prepare(
            gated,
            identity.clone(),
            RunOptions {
                presentation: false,
                validate: false,
            },
        )
        .err()
        .unwrap();
        assert!(
            error.contains("requires its final verification gate"),
            "{error}"
        );
        // Catalog preflight must run even when the archive is also malformed.
        let invalid = ReplayPackage::new(source(2, "wrong/group"), vec![0xff]).unwrap();
        let error = prepare(invalid, identity, options).err().unwrap();
        assert!(error.contains("belongs to"), "{error}");
    }
}

impl crate::CadServer {
    /// Called only by the project replay host after its admitted baseline has
    /// returned an exact successor publication receipt. Ordinary interface and
    /// inbox replacement paths keep their existing stop-on-replacement guards.
    pub(super) fn adopt_replay_successor(
        &mut self,
        previous: &limo_cad_project_file::PublicationReceipt,
        successor: &limo_cad_project_file::PublicationReceipt,
    ) -> Result<(), String> {
        if !self.script_running
            || self.failed_attachment.is_some()
            || self.attached_document_id.as_deref() != Some(previous.session_id.as_str())
            || self.loaded_publication.as_ref() != Some(previous)
            || self.attached_generation != Some(previous.engine_revision)
        {
            return Err("Replay no longer owns its original loaded publication".into());
        }
        validate_successor(previous, successor)?;
        if let Err(error) =
            self.load_snapshot_model_checked(&successor.session_id, false, Some(successor))
        {
            self.failed_attachment = Some(crate::AttachmentFailure {
                session_id: successor.session_id.clone(),
                error: error.clone(),
            });
            return Err(error);
        }
        self.attached_document_id = Some(successor.session_id.clone());
        self.apply_snapshot_focus(&successor.session_id);
        Ok(())
    }
}

fn validate_successor(
    previous: &limo_cad_project_file::PublicationReceipt,
    successor: &limo_cad_project_file::PublicationReceipt,
) -> Result<(), String> {
    if successor.process_instance_id != previous.process_instance_id
        || successor.window_id != previous.window_id
        || successor.document_id != previous.document_id
        || successor.session_id == previous.session_id
        || successor.epoch <= previous.epoch
        || successor.engine_revision == 0
        || successor.export_sequence == 0
    {
        return Err(
            "Replay baseline did not acknowledge a successor of its original document".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod successor_tests {
    use super::*;
    use limo_cad_project_file::PublicationReceipt;
    fn original() -> PublicationReceipt {
        PublicationReceipt {
            process_instance_id: "process".into(),
            session_id: "old".into(),
            window_id: "main".into(),
            document_id: "document".into(),
            epoch: 3,
            engine_revision: 5,
            export_sequence: 9,
        }
    }
    #[test]
    fn only_same_document_new_publisher_can_be_a_replay_successor() {
        let old = original();
        let next = PublicationReceipt {
            session_id: "next".into(),
            epoch: 7,
            engine_revision: 1,
            export_sequence: 1,
            ..old.clone()
        };
        validate_successor(&old, &next).unwrap();
        for case in 0..8 {
            let mut bad = next.clone();
            match case {
                0 => bad.process_instance_id = "other".into(),
                1 => bad.window_id = "other".into(),
                2 => bad.document_id = "other".into(),
                3 => bad.session_id = old.session_id.clone(),
                4 => bad.epoch = old.epoch,
                5 => bad.epoch = 2,
                6 => bad.engine_revision = 0,
                _ => bad.export_sequence = 0,
            }
            assert!(validate_successor(&old, &bad).is_err(), "{case}");
        }
    }
    #[test]
    fn successor_adoption_cannot_start_outside_an_owned_run() {
        let mut server = crate::CadServer::new().unwrap();
        let old = original();
        let next = PublicationReceipt {
            session_id: "next".into(),
            epoch: 7,
            engine_revision: 1,
            export_sequence: 1,
            ..old.clone()
        };
        let before = server.manager.export_project_snapshot().unwrap();
        assert!(server.adopt_replay_successor(&old, &next).is_err());
        assert!(server.attached_document_id.is_none());
        assert!(server.loaded_publication.is_none());
        assert_eq!(server.manager.export_project_snapshot().unwrap(), before);
    }
}

/// Run on a dedicated script thread, never on the native model worker. The
/// callback submits the prepared archive once and waits for its original native
/// acknowledgement. It must return the publication receipt from that committed
/// baseline, and must not resolve a later active session on timeout or failure.
pub fn run_prepared_project_replay<F>(
    prepared: PreparedProjectReplay,
    session_id: &str,
    speed: f64,
    commit_baseline: F,
) -> Result<serde_json::Value, String>
where
    F: FnOnce(
        ProjectArchive,
        limo_cad_project_file::PublicationReceipt,
        ReplayPackageIdentity,
    ) -> Result<limo_cad_project_file::PublicationReceipt, String>,
{
    use serde_json::json;
    if !speed.is_finite() || !(0.25..=16.0).contains(&speed) {
        return Err("Replay speed must be finite and between 0.25 and 16".into());
    }
    let (script, archive, options, identity) = prepared.into_parts();
    script.script().validate_options(options)?;
    crate::interface::validate_script(script.script())?;
    let mut server = crate::CadServer::new()?;
    server.attach_read_only_snapshot(&json!({"session_id":session_id}))?;
    let previous = server
        .loaded_publication
        .clone()
        .ok_or("Project replay requires a composite desktop publication")?;
    crate::session::require_current_replay_publication(&previous)?;
    let active_sketch = server.call_tool("sketch_active", json!({}))?;
    if !active_sketch.is_null() || !server.manager.is_blank_for_script() {
        return Err("Project replay requires a blank native document".into());
    }
    let step_count = script.script().metadata()["step_count"].clone();
    server.script_running = true;
    let configured = server.call_tool("cad_interface", json!({"action":"presentation","command":"configure",
        "mode":if options.presentation {"present"} else {"fast"},"speed":speed,"step_index":0,"step_count":step_count,"chapter":"","text":""}));
    let result = match configured {
        Ok(value) if value["status"] == "applied" => {
            let mut host = LiveProjectHost {
                server: &mut server,
                previous,
                identity,
                archive: Some(archive),
                commit: Some(commit_baseline),
            };
            script.run(&mut host, options)
        }
        Ok(value) => Err(format!(
            "Replay playback configuration was not acknowledged: {value}"
        )),
        Err(error) => Err(error),
    };
    // A project-backed source must never be advertised as a self-contained v1
    // authored script. The package exporter owns that future representation.
    server.finish_script_run(script.script(), None, step_count, false, result)
}

struct LiveProjectHost<'a, F> {
    server: &'a mut crate::CadServer,
    previous: limo_cad_project_file::PublicationReceipt,
    identity: ReplayPackageIdentity,
    archive: Option<ProjectArchive>,
    commit: Option<F>,
}
impl<F> limo_cad_script::ProjectReplayHost for LiveProjectHost<'_, F>
where
    F: FnOnce(
        ProjectArchive,
        limo_cad_project_file::PublicationReceipt,
        ReplayPackageIdentity,
    ) -> Result<limo_cad_project_file::PublicationReceipt, String>,
{
    fn validate_call(&mut self, group: &str, operation: &str) -> Result<(), String> {
        crate::interface::validate_script_call(group, operation)
    }
    fn load_project_baseline(&mut self) -> Result<(), String> {
        let commit = self
            .commit
            .take()
            .ok_or("Replay baseline was already submitted")?;
        let archive = self
            .archive
            .take()
            .ok_or("Replay baseline archive was already consumed")?;
        let successor = match commit(archive, self.previous.clone(), self.identity.clone()) {
            Ok(receipt) => receipt,
            Err(error) => {
                self.server.failed_attachment = Some(crate::AttachmentFailure {
                    session_id:self.previous.session_id.clone(),error:format!("Replay baseline was not acknowledged; inspect retained work before restarting: {error}") });
                return Err(error);
            }
        };
        self.server
            .adopt_replay_successor(&self.previous, &successor)
    }
    fn call(
        &mut self,
        name: &str,
        arguments: serde_json::Value,
        progress: limo_cad_script::RunProgress,
    ) -> Result<serde_json::Value, String> {
        self.server.dispatch_script_call(name, arguments, progress)
    }
}

#[cfg(test)]
mod live_host_tests {
    use super::*;
    use limo_cad_project_file::{PublicationReceipt, SaveMetadata};
    use limo_cad_script::ProjectReplayHost;
    use serde_json::json;
    fn prepared() -> PreparedProjectReplay {
        PreparedProjectReplay {
            script:ProjectScript::parse(&json!({"version":2,"name":"Host fixture","starting_state":"project","steps":[
                {"call":{"group":"document/files","operation":"cad_set_document_name","arguments":{"name":"Must not run"}}}
            ]}).to_string()).unwrap(),
            archive:ProjectArchive::new(r#"{"format":"limo-cad-project","schema_version":15}"#.into(),SaveMetadata {application_version:"test",saved_at:"2026-10-17T00:00:00Z"}).unwrap(),
            options:RunOptions {presentation:false,validate:true},
            identity:ReplayPackageIdentity {byte_length:1,sha256:[0;32]},
        }
    }
    #[test]
    fn invalid_speed_fails_before_host_or_baseline_submission() {
        for speed in [f64::NAN, f64::INFINITY, 0.0, 0.24, 16.01] {
            let called = std::cell::Cell::new(false);
            let result =
                run_prepared_project_replay(prepared(), "unused-session", speed, |_, _, _| {
                    called.set(true);
                    Err("must not run".into())
                });
            assert!(result.unwrap_err().contains("speed"));
            assert!(!called.get());
        }
    }
    #[test]
    fn failed_baseline_is_one_shot_and_blocks_later_steps() {
        let mut server = crate::CadServer::new().unwrap();
        server.script_running = true;
        let before = server.manager.export_project_snapshot().unwrap();
        let (script, archive, options, identity) = prepared().into_parts();
        let retained = archive.encode().unwrap();
        let previous = PublicationReceipt {
            process_instance_id: "process".into(),
            session_id: "original".into(),
            window_id: "main".into(),
            document_id: "document".into(),
            epoch: 1,
            engine_revision: 1,
            export_sequence: 1,
        };
        server.attached_document_id = Some(previous.session_id.clone());
        let calls = std::cell::Cell::new(0);
        let commit = |archive: ProjectArchive,
                      receipt: PublicationReceipt,
                      received: ReplayPackageIdentity| {
            calls.set(calls.get() + 1);
            assert_eq!(archive.encode().unwrap(), retained);
            assert_eq!(receipt, previous);
            assert_eq!(received, identity);
            Err("Original acknowledgement unavailable".into())
        };
        let mut host = LiveProjectHost {
            server: &mut server,
            previous: previous.clone(),
            identity: identity.clone(),
            archive: Some(archive),
            commit: Some(commit),
        };
        let result = script.run(&mut host, options);
        assert!(result
            .as_ref()
            .unwrap_err()
            .contains("acknowledgement unavailable"));
        assert!(host
            .load_project_baseline()
            .unwrap_err()
            .contains("already submitted"));
        assert_eq!(calls.get(), 1);
        assert_eq!(
            host.server.manager.export_project_snapshot().unwrap(),
            before
        );
        assert!(host.server.failed_attachment.is_some());
        drop(host);
        server.live_snapshot_dirty = true;
        assert!(server
            .finish_script_run(script.script(), None, json!(1), false, result)
            .is_err());
        assert!(!server.script_running);
        assert!(server.failed_attachment.is_some());
        assert_eq!(server.manager.export_project_snapshot().unwrap(), before);
        assert!(server.last_script_source.is_none());
    }
}
