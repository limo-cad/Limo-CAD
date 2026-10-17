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
