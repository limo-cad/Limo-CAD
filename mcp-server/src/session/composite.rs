use super::*;
use limo_cad_project_file::{ProjectPublication, PublicationReceipt};

/// Composite publication is opt-in. Once committed or advertised, failures
/// must never fall back to a legacy model that omits domain intent.
pub fn read_composite_publication(session_id: &str) -> Result<Option<ProjectPublication>, String> {
    let marker = session_path(session_id, "publication-commit.cbor")?;
    let heartbeat = read_heartbeat_snapshot(session_id).unwrap_or(json!({}));
    if heartbeat.get("publication_version").is_none() {
        return match limo_cad_session_storage::read_bounded(&marker, 4096) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(format!("Could not inspect composite publication: {error}")),
            Ok(_) => {
                Err("Composite publication is not yet acknowledged by the desktop heartbeat".into())
            }
        };
    }
    if heartbeat["publication_version"] != 1 {
        return Err("Unsupported composite publication version".into());
    }
    let expected: PublicationReceipt =
        serde_json::from_value(heartbeat["publication_receipt"].clone())
            .map_err(|error| format!("Invalid composite publication receipt: {error}"))?;
    if expected.session_id != session_id
        || heartbeat["session_id"] != expected.session_id
        || heartbeat["process_instance_id"] != expected.process_instance_id
        || heartbeat["window_id"] != expected.window_id
        || heartbeat["document_id"] != expected.document_id
        || heartbeat["model_generation"].as_u64() != Some(expected.engine_revision)
    {
        return Err("Composite publication receipt differs from its desktop route".into());
    }
    let publication = limo_cad_session_publication::read_committed(
        marker
            .parent()
            .ok_or("Publication has no session directory")?,
    )
    .map_err(|error| format!("Could not read composite publication: {error}"))?;
    if publication.receipt() != &expected
        || read_heartbeat_snapshot(session_id).as_ref() != Some(&heartbeat)
    {
        return Err(
            "Composite publication changed during refresh; retry after the desktop publishes"
                .into(),
        );
    }
    Ok(Some(publication))
}

/// Replay accepts only the explicitly acknowledged successor, never the latest
/// available snapshot or a legacy fallback. Receipt equality is not admission.
pub(crate) fn read_replay_publication(
    expected: &PublicationReceipt,
) -> Result<ProjectPublication, String> {
    require_current_replay_publication(expected)?;
    let publication = read_composite_publication(&expected.session_id)?
        .ok_or("Replay requires a composite publication")?;
    if publication.receipt() != expected {
        return Err("Replay successor publication differs from its acknowledgement".into());
    }
    require_current_replay_publication(expected)?;
    Ok(publication)
}

/// Recheck after potentially slow kernel preparation, before local commit. This
/// is observation, not a cross-process lock; later operations still use OCC.
pub(crate) fn require_current_replay_publication(
    expected: &PublicationReceipt,
) -> Result<(), String> {
    require_valid_session_id(&expected.session_id)?;
    require_open_session(&expected.session_id)?;
    let heartbeat = read_heartbeat_snapshot(&expected.session_id)
        .ok_or("Replay successor heartbeat is unavailable")?;
    validate_replay_owner(expected, &heartbeat, &process_registry())
}

fn validate_replay_owner(
    expected: &PublicationReceipt,
    heartbeat: &Value,
    registry: &ProcessRegistry,
) -> Result<(), String> {
    let actual: PublicationReceipt =
        serde_json::from_value(heartbeat["publication_receipt"].clone())
            .map_err(|e| format!("Invalid replay publication receipt: {e}"))?;
    let lease = registry
        .leases
        .get(&expected.process_instance_id)
        .ok_or("Replay desktop process lease is unavailable")?;
    let active = lease
        .windows
        .get(&expected.window_id)
        .ok_or("Replay desktop window is unavailable")?;
    if actual != *expected
        || heartbeat["publication_version"] != 1
        || heartbeat["interface_version"] != 1
        || heartbeat["session_id"] != expected.session_id
        || heartbeat["process_instance_id"] != expected.process_instance_id
        || heartbeat["window_id"] != expected.window_id
        || heartbeat["document_id"] != expected.document_id
        || heartbeat["generation"].as_u64() != Some(expected.engine_revision)
        || heartbeat["model_generation"].as_u64() != Some(expected.engine_revision)
        || heartbeat_meta_from_snapshot(Some(heartbeat))["stale"] != false
        || lease.process_instance_id != expected.process_instance_id
        || active.active_session_id != expected.session_id
        || active.active_document_id != expected.document_id
    {
        return Err("Replay successor is no longer the exact active published document".into());
    }
    Ok(())
}

#[cfg(test)]
mod replay_tests {
    use super::*;
    fn fixture() -> (PublicationReceipt, Value, ProcessRegistry) {
        let receipt = PublicationReceipt {
            process_instance_id: "process".into(),
            session_id: "successor".into(),
            window_id: "main".into(),
            document_id: "document".into(),
            epoch: 8,
            engine_revision: 1,
            export_sequence: 1,
        };
        let heartbeat = json!({"publication_version":1,"interface_version":1,"publication_receipt":receipt,
            "session_id":"successor","process_instance_id":"process","window_id":"main","document_id":"document",
            "generation":1,"model_generation":1,"updated_ms":now_ms()});
        let mut registry = ProcessRegistry::default();
        registry.leases.insert(
            "process".into(),
            ProcessLease {
                process_instance_id: "process".into(),
                pid: Some(1),
                updated_ms: now_ms(),
                windows: BTreeMap::from([(
                    "main".into(),
                    ActiveWindowLease {
                        active_document_id: "document".into(),
                        active_session_id: "successor".into(),
                    },
                )]),
                accepts_unlisted_windows: false,
            },
        );
        (receipt, heartbeat, registry)
    }
    #[test]
    fn exact_replay_owner_rejects_retained_or_advanced_publications() {
        let (receipt, good, registry) = fixture();
        validate_replay_owner(&receipt, &good, &registry).unwrap();
        for (key, value) in [
            ("publication_version", json!(2)),
            ("interface_version", json!(0)),
            ("session_id", json!("old")),
            ("process_instance_id", json!("other")),
            ("window_id", json!("other")),
            ("document_id", json!("other")),
            ("generation", json!(2)),
            ("model_generation", json!(2)),
            ("updated_ms", json!(0)),
        ] {
            let mut bad = good.clone();
            bad[key] = value;
            assert!(
                validate_replay_owner(&receipt, &bad, &registry).is_err(),
                "{key}"
            );
        }
        for key in ["epoch", "engine_revision", "export_sequence"] {
            let mut bad = good.clone();
            bad["publication_receipt"][key] = json!(99);
            assert!(
                validate_replay_owner(&receipt, &bad, &registry).is_err(),
                "{key}"
            );
        }
        assert!(validate_replay_owner(&receipt, &good, &ProcessRegistry::default()).is_err());
        for changed_document in [false, true] {
            let (_, _, mut registry) = fixture();
            let active = registry
                .leases
                .get_mut("process")
                .unwrap()
                .windows
                .get_mut("main")
                .unwrap();
            if changed_document {
                active.active_document_id = "other".into();
            } else {
                active.active_session_id = "other".into();
            }
            assert!(validate_replay_owner(&receipt, &good, &registry).is_err());
        }
    }
}
