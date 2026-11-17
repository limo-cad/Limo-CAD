//! Baseline work uses the existing ordered model worker. The script runner
//! waits on a separate acknowledgement channel and must never occupy that worker.
use super::super::super::presentation;
use super::*;
use limo_cad_project_file::{ProjectArchive, PublicationReceipt};

pub(in super::super) mod launch;

type Acknowledgement = mpsc::Receiver<Result<PublicationReceipt, String>>;

/// The caller retains its prepared script/package identity and registers Running
/// before enqueueing. This consumes the already-prepared archive exactly once.
/// A committed but unacknowledged baseline must never be submitted again.
pub(in super::super) fn enqueue_baseline(
    world: &mut World,
    expected: DocumentReceipt,
    source_generation: u64,
    archive: ProjectArchive,
) -> Result<(Value, Acknowledgement), String> {
    validate_replay_lesson(world, source_generation, &expected.owner)?;
    presentation::require_replay_ready(world, &expected.owner)?;
    let workspace = world.resource::<Files>().workspace.clone();
    let (handoff_send, handoff_receive) = mpsc::sync_channel(1);
    let (acknowledge, receipt) = mpsc::channel();
    let queued = worker::enqueue_transaction(
        world,
        "replay_baseline".into(),
        move |services, guard| {
            guard.validate_preparation()?;
            let (result, handoff) = workspace
                .lock()
                .map_err(|_| "Document workspace lock poisoned")?
                .load_replay_baseline_guarded(
                    &services.bridge,
                    &services.engine,
                    &expected,
                    archive,
                    || guard.validate(),
                )?;
            handoff_send
                .send(handoff)
                .map_err(|_| "Replay baseline committed but its completion receiver retired")?;
            Ok(result)
        },
        move |world, services, result| {
            let completion = (|| {
                let result = result?;
                let handoff = handoff_receive
                    .try_recv()
                    .map_err(|_| "Replay baseline returned no committed handoff")?;
                let successor = handoff.successor().clone();
                if result.context != successor.owner || result.engine_revision != successor.revision
                {
                    return Err(
                        "Replay baseline result does not match its committed handoff".into(),
                    );
                }
                // Transfer before finish_document_transition or the controller's
                // normal observe pass can reset state for the replacement epoch.
                acknowledge_replay_baseline(world, services, source_generation, &handoff)?;
                let output = finish_document_transition(world, services, "replay_baseline", result);
                let publication = validate_presented_baseline(&output, &successor)?;
                services.bridge.with_native_document_receipt(&services.engine, &successor.owner, |revision| {
                    if revision != successor.revision {
                        return Err("Replay baseline changed before runner acknowledgement".into());
                    }
                    if presentation::gate(world, &successor.owner) == presentation::Gate::Stopped {
                        return Err("Replay stopped during baseline commit; the loaded design is preserved".into());
                    }
                    Ok(())
                })?;
                Ok((output, publication))
            })();
            let answer = completion
                .as_ref()
                .map(|(_, receipt)| receipt.clone())
                .map_err(Clone::clone);
            if acknowledge.send(answer).is_err() {
                return Err("Replay baseline completion receiver retired; inspect retained work before restarting".into());
            }
            completion.map(|(output, _)| output)
        },
    )?;
    Ok((queued, receipt))
}

fn validate_presented_baseline(
    output: &Value,
    expected: &DocumentReceipt,
) -> Result<PublicationReceipt, String> {
    if output["document_id"] != expected.owner.document_id
        || output["document_epoch"].as_u64() != Some(expected.owner.epoch)
        || output["engine_revision"].as_u64() != Some(expected.revision)
        || output["result"]["replay_baseline_loaded"] != true
        || output["publication_pending"] != false
        || output["publication"].is_null()
        || output["presentation_pending"] == true
        || ["render_error", "publication_error", "presentation_error"]
            .iter()
            .any(|key| !output[*key].is_null())
    {
        return Err(format!("Replay baseline committed but presentation/publication was not acknowledged; preserve the loaded design: {output}"));
    }
    let publication = &output["publication"];
    let receipt: PublicationReceipt =
        serde_json::from_value(publication["publication_receipt"].clone()).map_err(|error| {
            format!("Replay baseline has no valid publication receipt: {error}")
        })?;
    if publication["skipped"] != false
        || publication["publication_version"] != 1
        || receipt.window_id != expected.owner.window_id
        || receipt.document_id != expected.owner.document_id
        || receipt.epoch != expected.owner.epoch
        || receipt.engine_revision != expected.revision
        || receipt.session_id.is_empty()
        || receipt.process_instance_id.is_empty()
        || receipt.export_sequence == 0
        || publication["session_id"] != receipt.session_id
        || publication["window_id"] != receipt.window_id
        || publication["document_id"] != receipt.document_id
        || publication["model_generation"].as_u64() != Some(receipt.engine_revision)
    {
        return Err("Replay publication receipt does not match its committed baseline".into());
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acknowledgement_requires_matching_presented_and_published_baseline() {
        let receipt = DocumentReceipt {
            owner: DocumentContext {
                window_id: "main".into(),
                document_id: "replay".into(),
                epoch: 7,
            },
            revision: 1,
        };
        let published = PublicationReceipt {
            process_instance_id: "process".into(),
            session_id: "successor".into(),
            window_id: "main".into(),
            document_id: "replay".into(),
            epoch: 7,
            engine_revision: 1,
            export_sequence: 3,
        };
        let good = json!({"document_id":"replay","document_epoch":7,"engine_revision":1,
            "result":{"replay_baseline_loaded":true},"publication_pending":false,
            "publication":{"skipped":false,"publication_version":1,"publication_receipt":published,
                "session_id":"successor","window_id":"main","document_id":"replay","model_generation":1}});
        assert_eq!(
            validate_presented_baseline(&good, &receipt).unwrap(),
            published
        );
        for (key, value) in [
            ("skipped", json!(true)),
            ("publication_version", json!(2)),
            ("session_id", json!("other")),
            ("window_id", json!("other")),
            ("document_id", json!("other")),
            ("model_generation", json!(2)),
            ("publication_receipt", Value::Null),
        ] {
            let mut failed = good.clone();
            failed["publication"][key] = value;
            assert!(
                validate_presented_baseline(&failed, &receipt).is_err(),
                "{key}"
            );
        }
        for (key, value) in [
            ("process_instance_id", json!("")),
            ("session_id", json!("")),
            ("window_id", json!("other")),
            ("document_id", json!("other")),
            ("epoch", json!(8)),
            ("engine_revision", json!(2)),
            ("export_sequence", json!(0)),
        ] {
            let mut failed = good.clone();
            failed["publication"]["publication_receipt"][key] = value;
            assert!(
                validate_presented_baseline(&failed, &receipt).is_err(),
                "{key}"
            );
        }
        assert!(validate_presented_baseline(&Value::Null, &receipt).is_err());
        for (key, value) in [
            ("document_id", json!("other")),
            ("document_epoch", json!(8)),
            ("engine_revision", json!(2)),
            ("result", json!({})),
            ("publication", Value::Null),
            ("publication_pending", json!(true)),
            ("presentation_pending", json!(true)),
            ("render_error", json!("render failed")),
            ("publication_error", json!("publish failed")),
            ("presentation_error", json!("camera failed")),
        ] {
            let mut failed = good.clone();
            failed[key] = value;
            assert!(
                validate_presented_baseline(&failed, &receipt).is_err(),
                "{key}"
            );
        }
    }
}
