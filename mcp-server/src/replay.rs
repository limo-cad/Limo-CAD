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
