//! Baseline work uses the existing ordered model worker. The script runner
//! waits on a separate acknowledgement channel and must never occupy that worker.
use super::super::super::presentation;
use super::*;
use limo_cad_project_file::ProjectArchive;

type Acknowledgement = mpsc::Receiver<Result<DocumentReceipt, String>>;

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
                validate_presented_baseline(&output, &successor)?;
                services.bridge.with_native_document_receipt(&services.engine, &successor.owner, |revision| {
                    if revision != successor.revision {
                        return Err("Replay baseline changed before runner acknowledgement".into());
                    }
                    if presentation::gate(world, &successor.owner) == presentation::Gate::Stopped {
                        return Err("Replay stopped during baseline commit; the loaded design is preserved".into());
                    }
                    Ok(())
                })?;
                Ok((output, successor))
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

fn validate_presented_baseline(output: &Value, expected: &DocumentReceipt) -> Result<(), String> {
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
    Ok(())
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
        let good = json!({"document_id":"replay","document_epoch":7,"engine_revision":1,
            "result":{"replay_baseline_loaded":true},"publication_pending":false,"publication":{}});
        validate_presented_baseline(&good, &receipt).unwrap();
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
