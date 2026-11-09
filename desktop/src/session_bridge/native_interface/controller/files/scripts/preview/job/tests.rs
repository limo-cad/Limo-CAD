use super::*;
use limo_cad_project_file::{ProjectArchive, ReplayPackage, SaveMetadata};

pub(in super::super) fn source_text() -> String {
    json!({"version":2,"name":"Bounded package admission","starting_state":"project","steps":[]})
        .to_string()
}

pub(in super::super) fn valid_archive() -> Vec<u8> {
    ProjectArchive::new(
        limo_cad_sketch::SketchManager::new()
            .export_project_model()
            .unwrap(),
        SaveMetadata {
            application_version: "test",
            saved_at: "2026-10-23T00:00:00Z",
        },
    )
    .unwrap()
    .encode()
    .unwrap()
}

/// An attacker-controlled expanded-size declaration, checked before inflation.
/// The fixture remains a few hundred bytes; it allocates no expanded payload.
pub(in super::super) fn declared_zip_bomb() -> Vec<u8> {
    let mut bytes = valid_archive();
    let index = bytes.windows(4).position(|v| v == b"PK\x01\x02").unwrap();
    bytes[index + 24..index + 28].copy_from_slice(&(32_u32 * 1024 * 1024 + 1).to_le_bytes());
    assert!(bytes.len() < 4096);
    bytes
}

pub(in super::super) fn loaded(source: &str, bytes: Vec<u8>) -> Arc<Loaded> {
    let package = Arc::new(ReplayPackage::new(source.to_owned(), bytes).unwrap());
    let identity = package.source_identity(source).unwrap();
    Arc::new(Loaded {
        artifact: Artifact::Project {
            package,
            opened: identity.clone(),
            saved: identity,
        },
        // A read of this nonexistent path would replace the admission result.
        path: Some(std::env::temp_dir().join(format!(
            "unopened-{}.limo.replay.cbor",
            uuid::Uuid::new_v4()
        ))),
        example: None,
        name: "Retained admission fixture".into(),
        counts: Some((0, 0)),
        maximum: limo_cad_mcp::MAX_SCRIPT_BYTES,
        inspection: json!({"source":source,"authored_source":source}),
    })
}

#[test]
fn native_project_source_rejects_malformed_oversized_and_zip_bomb_before_runtime() {
    let source = source_text();
    // Two MiB of whitespace exercises the real source bound without a large
    // disk fixture or an allocated 16 MiB archive/32 MiB inflated document.
    let oversized = format!("{}{source}", " ".repeat(2 * 1024 * 1024));
    for (source, bytes, expected) in [
        (
            source.clone(),
            b"PK\x03\x04damaged".to_vec(),
            "ZIP is damaged",
        ),
        (oversized, valid_archive(), "2 MiB"),
        (
            source.clone(),
            declared_zip_bomb(),
            "33554432 byte safety limit",
        ),
    ] {
        let loaded = loaded(&source, bytes);
        let path = loaded.path.clone().unwrap();
        assert!(!path.exists());
        let Artifact::Project { package, .. } = &loaded.artifact else {
            unreachable!()
        };
        let original = package.project_bytes().to_vec();
        let cache = Mutex::new(prepared::Cache::default());
        let before = cache.lock().unwrap().metrics();
        let mut checkpoints = 0;
        let error = Source::Project(Arc::clone(&loaded))
            .prepare(Locale::En, &cache, || {
                checkpoints += 1;
                assert_eq!(
                    checkpoints, 1,
                    "Rejected input crossed admission toward the detached runtime"
                );
                Ok(())
            })
            .err()
            .unwrap();
        assert!(error.contains(expected), "{error}");
        assert_eq!(checkpoints, 1);
        assert_eq!(
            cache.lock().unwrap().metrics(),
            before,
            "Package rejection cannot enter or evict the authored-guide LRU"
        );
        assert_eq!(package.project_bytes(), original);
        assert_eq!(loaded.authored(), source);
        assert!(!path.exists());
        assert_eq!(
            Arc::strong_count(&loaded),
            1,
            "The failed job retained its source"
        );
    }
}

#[test]
fn native_project_source_honors_cancellation_at_admission_and_before_runtime() {
    let source = source_text();
    let loaded = loaded(&source, valid_archive());
    let cache = Mutex::new(prepared::Cache::default());
    for rejected_checkpoint in [1, 2] {
        let mut checkpoints = 0;
        let error = Source::Project(Arc::clone(&loaded))
            .prepare(Locale::En, &cache, || {
                checkpoints += 1;
                if checkpoints == rejected_checkpoint {
                    Err("test cancellation fence".into())
                } else {
                    Ok(())
                }
            })
            .err()
            .unwrap();
        assert_eq!(error, "test cancellation fence");
        assert_eq!(checkpoints, rejected_checkpoint);
        assert_eq!(Arc::strong_count(&loaded), 1);
    }
    assert_eq!(cache.lock().unwrap().metrics(), (0, 0, 0, 0));
}
