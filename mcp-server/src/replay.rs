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

fn prepare(
    package: ReplayPackage,
    identity: ReplayPackageIdentity,
    options: RunOptions,
) -> Result<PreparedProjectReplay, String> {
    // Validate every declared call/check before archive decoding or live work.
    let script = ProjectScript::parse(package.script())?;
    script.script().validate_options(options)?;
    crate::interface::validate_script(script.script())?;
    let (_, archive) = package.into_project().map_err(|e| e.to_string())?;
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
