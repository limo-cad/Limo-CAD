//! Loaded scripts use the shared live runner. The runner
//! waits for inbox receipts, so it must never occupy the modeling worker.
use super::*;

pub(super) struct Running {
    owner: DocumentContext,
    source_generation: u64,
    result: Mutex<mpsc::Receiver<Result<Value, String>>>,
}

pub(super) fn start_source_with_options(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    (services, owner): (&NativeServices, &DocumentContext),
    name: &str,
    source: String,
    (mode, speed): (&'static str, f64),
) -> Result<(), String> {
    if world.resource::<Files>().script.preview.building() {
        return Err("Wait for the isolated lesson preview to finish preparing".into());
    }
    if world.resource::<Files>().lesson.is_some() {
        return Err(
            "A script is already running. Use its playback controls to stop or pause it".into(),
        );
    }
    if awaiting(world) {
        return Err("Finish the current File dialog first".into());
    }
    let receipt =
        services
            .bridge
            .with_native_document_receipt(&services.engine, owner, |revision| {
                if !services.engine.is_blank_for_script() {
                    return Err("Scripts require a blank document. Use New document first".into());
                }
                Ok(DocumentReceipt {
                    owner: owner.clone(),
                    revision,
                })
            })?;
    let session = services
        .bridge
        .session_id_for_window(&owner.window_id)?
        .ok_or("Publish the current document before running a script")?;
    let session = services.bridge.active_script_session(
        &owner.window_id,
        &services.engine,
        &owner.document_id,
        &session,
    )?;
    let (send, receive) = mpsc::channel();
    let services = services.clone();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-native-script".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if services
                    .bridge
                    .native_document_receipt(&services.engine, &receipt.owner)?
                    != receipt
                {
                    return Err("The document changed before the script started".into());
                }
                limo_cad_mcp::run_script(&source, None, Some(&session), mode, speed)
            }))
            .unwrap_or_else(|_| {
                Err(
                    "The script worker stopped unexpectedly; any completed work is preserved"
                        .into(),
                )
            });
            let _ = send.send(result);
            wake.request_redraw();
        })
        .map_err(|error| format!("Cannot start script: {error}"))?;
    let mut files = world.resource_mut::<Files>();
    let source_generation = files.script.generation;
    files.lesson = Some(Running {
        owner: owner.clone(),
        source_generation,
        result: Mutex::new(receive),
    });
    files.script.status = Some(format!("Running {name}"));
    files.scripts = false;
    Ok(())
}

pub(super) fn poll(world: &mut World, services: &NativeServices) {
    let result = world
        .resource::<Files>()
        .lesson
        .as_ref()
        .and_then(|running| {
            Some(match running.result.lock() {
                Ok(receiver) => match receiver.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => return None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err("The script worker disconnected".into())
                    }
                },
                Err(_) => Err("The script result could not be read".into()),
            })
        });
    let Some(result) = result else { return };
    let running = world.resource_mut::<Files>().lesson.take().unwrap();
    if world.resource::<Files>().script.generation != running.source_generation {
        return;
    }
    let same_owner = tabs(world, services, &running.owner)
        .is_ok_and(|tabs| tabs.iter().any(|tab| tab.owner == running.owner));
    let status = match result {
        Ok(_) if !same_owner => {
            "Script finished in a document that has since closed or changed; inspect retained work before running again".into()
        }
        Ok(report) => format!(
            "Script complete: {} steps, {} checks",
            report["steps_completed"], report["checks_completed"]
        ),
        Err(error) => format!("Script stopped: {error}"),
    };
    world.resource_mut::<Files>().script.status = Some(status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lesson_refuses_work_before_starting_any_worker() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = super::super::super::super::tests::Fixture::new();
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let mut world = World::new();
        initialize(
            &mut world,
            Arc::new(Mutex::new(DocumentWorkspace::default())),
        );
        let owner = fixture.owner();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &owner,
                "sketch_begin",
                &json!({"type":"origin_plane","plane":"xy"}),
                || Ok(()),
            )
            .unwrap();
        let before = fixture.engine.engine_call("project_export_model", "");
        let error = start_source_with_options(
            &mut world,
            &NativeInterfaceHandle::new(|| {}),
            (&services, &owner),
            "Blank-document guard",
            String::new(),
            ("present", 1.),
        )
        .unwrap_err();
        assert!(error.contains("blank"));
        assert!(world.resource::<Files>().lesson.is_none());
        assert_eq!(
            before,
            fixture.engine.engine_call("project_export_model", "")
        );
    }
}
