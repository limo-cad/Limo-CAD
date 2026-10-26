//! UI-owned bridge between the dedicated script runner and ordered model worker.
use super::*;
use limo_cad_project_file::ReplayPackageIdentity;

type Answer = mpsc::Sender<Result<PublicationReceipt, String>>;
struct BaselineRequest {
    archive: ProjectArchive,
    previous: PublicationReceipt,
    identity: ReplayPackageIdentity,
    answer: Answer,
}
pub(in super::super::super) struct Launch {
    original: DocumentReceipt,
    session: String,
    process: String,
    identity: ReplayPackageIdentity,
    source_generation: u64,
    incoming: Option<mpsc::Receiver<BaselineRequest>>,
    waiting: Option<BaselineRequest>,
    acknowledgement: Option<(Acknowledgement, Answer)>,
}

pub(in super::super::super) fn start_prepared(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
    prepared: limo_cad_mcp::PreparedProjectReplay,
    speed: f64,
) -> Result<(), String> {
    require_idle_model(world)?;
    let (original, session) = prepare_start(world, services, owner)?;
    let generation = world.resource::<Files>().script.generation;
    let name = prepared.script().script().metadata()["name"]
        .as_str()
        .unwrap_or("Project replay")
        .to_owned();
    let identity = prepared.identity().clone();
    let (request, incoming) = mpsc::sync_channel(1);
    let (send, receive) = mpsc::channel();
    let launch = Launch {
        original: original.clone(),
        session: session.clone(),
        process: services.bridge.process_instance_id.clone(),
        identity,
        source_generation: generation,
        incoming: Some(incoming),
        waiting: None,
        acknowledgement: None,
    };
    let services = services.clone();
    let wake = handle.clone();
    std::thread::Builder::new().name("cad-native-project-script".into()).spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if services.bridge.native_document_receipt(&services.engine,&original.owner)? != original {
                return Err("The document changed before project replay started".into());
            }
            limo_cad_mcp::run_prepared_project_replay(prepared,&session,speed,|archive,previous,identity| {
                let (answer,acknowledgement)=mpsc::channel();
                request.send(BaselineRequest {archive,previous,identity,answer}).map_err(|_| "Native replay launch retired before baseline submission")?;
                wake.request_redraw();
                acknowledgement.recv().map_err(|_| "Native baseline acknowledgement disconnected; inspect retained work before restarting")?
            })
        })).unwrap_or_else(|_| Err("The project replay worker stopped unexpectedly; completed work is preserved".into()));
        let _ = send.send(result);
        wake.request_redraw();
    }).map_err(|error| format!("Cannot start project replay: {error}"))?;
    let mut files = world.resource_mut::<Files>();
    files.lesson = Some(Running {
        owner: owner.clone(),
        source_generation: generation,
        result: Mutex::new(receive),
        replay: Some(Mutex::new(launch)),
    });
    files.script.status = Some(format!("Running {name}"));
    files.scripts = false;
    Ok(())
}

pub(in super::super::super) fn poll(world: &mut World, _services: &NativeServices) {
    if worker::busy(world) {
        return;
    }
    let launch = world
        .resource_mut::<Files>()
        .lesson
        .as_mut()
        .and_then(|running| running.replay.take());
    let Some(launch) = launch else { return };
    let mut launch = launch
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    launch.advance(world);
    if let Some(running) = world.resource_mut::<Files>().lesson.as_mut() {
        running.replay = Some(Mutex::new(launch));
    }
}
impl Launch {
    fn validate_request(&self, request: &BaselineRequest) -> Result<(), String> {
        let previous = &request.previous;
        if request.identity != self.identity
            || previous.process_instance_id != self.process
            || previous.session_id != self.session
            || previous.window_id != self.original.owner.window_id
            || previous.document_id != self.original.owner.document_id
            || previous.epoch != self.original.owner.epoch
            || previous.engine_revision != self.original.revision
        {
            return Err(
                "Replay baseline request differs from the prepared package or original document"
                    .into(),
            );
        }
        Ok(())
    }
    fn advance(&mut self, world: &mut World) {
        if let Some((receiver, answer)) = self.acknowledgement.take() {
            match receiver.try_recv() {
                Ok(result) => {
                    let result = result.and_then(|publication| {
                        let owner = DocumentContext {
                            window_id: publication.window_id.clone(),
                            document_id: publication.document_id.clone(),
                            epoch: publication.epoch,
                        };
                        validate_replay_lesson(world, self.source_generation, &owner)?;
                        Ok(publication)
                    });
                    let _ = answer.send(result);
                }
                Err(mpsc::TryRecvError::Empty) => self.acknowledgement = Some((receiver, answer)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _=answer.send(Err("Replay baseline completion disconnected; inspect retained work before restarting".into()));
                }
            }
            return;
        }
        if self.waiting.is_none() {
            let Some(receiver) = self.incoming.as_ref() else {
                return;
            };
            match receiver.try_recv() {
                Ok(request) => {
                    self.waiting = Some(request);
                    self.incoming = None;
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.incoming = None;
                    return;
                }
            }
        }
        let request = self.waiting.take().unwrap();
        let ready = (|| {
            self.validate_request(&request)?;
            validate_replay_lesson(world, self.source_generation, &self.original.owner)?;
            presentation::replay_readiness(world, &self.original.owner)
        })();
        match ready {
            Ok(presentation::Gate::Waiting) => {
                self.waiting = Some(request);
            }
            Ok(presentation::Gate::Ready) => {
                let BaselineRequest {
                    archive, answer, ..
                } = request;
                match enqueue_baseline(
                    world,
                    self.original.clone(),
                    self.source_generation,
                    archive,
                ) {
                    Ok((_, receipt)) => self.acknowledgement = Some((receipt, answer)),
                    Err(error) => {
                        let _ = answer.send(Err(error));
                    }
                }
            }
            Ok(presentation::Gate::Stopped) => {
                let _ = request
                    .answer
                    .send(Err("Replay stopped before baseline submission".into()));
            }
            Err(error) => {
                let _ = request.answer.send(Err(error));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        Launch,
        BaselineRequest,
        mpsc::Receiver<Result<PublicationReceipt, String>>,
    ) {
        let identity = ReplayPackageIdentity {
            byte_length: 7,
            sha256: [1; 32],
        };
        let owner = DocumentContext {
            window_id: "main".into(),
            document_id: "document".into(),
            epoch: 3,
        };
        let original = DocumentReceipt { owner, revision: 1 };
        let launch = Launch {
            original: original.clone(),
            session: "original".into(),
            process: "process".into(),
            identity: identity.clone(),
            source_generation: 4,
            incoming: None,
            waiting: None,
            acknowledgement: None,
        };
        let (answer, receive) = mpsc::channel();
        let archive = ProjectArchive::new(
            r#"{"format":"limo-cad-project","schema_version":15}"#.into(),
            SaveMetadata {
                application_version: "test",
                saved_at: "2026-10-17T00:00:00Z",
            },
        )
        .unwrap();
        let previous = PublicationReceipt {
            process_instance_id: "process".into(),
            session_id: "original".into(),
            window_id: "main".into(),
            document_id: "document".into(),
            epoch: 3,
            engine_revision: 1,
            export_sequence: 1,
        };
        (
            launch,
            BaselineRequest {
                archive,
                previous,
                identity,
                answer,
            },
            receive,
        )
    }
    #[test]
    fn mismatched_baseline_is_rejected_before_any_world_or_worker_access() {
        let (launch, request, _) = fixture();
        launch.validate_request(&request).unwrap();
        for case in 0..8 {
            let (mut launch, mut request, receive) = fixture();
            match case {
                0 => request.identity.byte_length += 1,
                1 => request.identity.sha256[0] ^= 1,
                2 => request.previous.process_instance_id = "other".into(),
                3 => request.previous.session_id = "other".into(),
                4 => request.previous.window_id = "other".into(),
                5 => request.previous.document_id = "other".into(),
                6 => request.previous.epoch += 1,
                _ => request.previous.engine_revision += 1,
            }
            let (send, incoming) = mpsc::channel();
            assert!(send.send(request).is_ok());
            launch.incoming = Some(incoming);
            let mut world = World::new();
            launch.advance(&mut world);
            assert!(receive.try_recv().unwrap().is_err(), "{case}");
            assert!(
                launch.incoming.is_none()
                    && launch.waiting.is_none()
                    && launch.acknowledgement.is_none()
            );
            launch.advance(&mut world);
            assert!(receive.try_recv().is_err());
        }
    }
    #[test]
    fn acknowledgement_forwards_exact_original_result_and_disconnect_never_retries() {
        for disconnect in [false, true] {
            let (mut launch, request, receive) = fixture();
            let expected = request.previous.clone();
            let (complete, acknowledgement) = mpsc::channel();
            launch.acknowledgement = Some((acknowledgement, request.answer));
            let mut world = World::new();
            initialize(
                &mut world,
                Arc::new(Mutex::new(DocumentWorkspace::default())),
            );
            let (_result_send, result_receive) = mpsc::channel();
            world.resource_mut::<Files>().script.generation = launch.source_generation;
            world.resource_mut::<Files>().lesson = Some(Running {
                owner: launch.original.owner.clone(),
                source_generation: launch.source_generation,
                result: Mutex::new(result_receive),
                replay: None,
            });
            launch.advance(&mut world);
            assert!(launch.acknowledgement.is_some());
            if !disconnect {
                complete.send(Ok(expected.clone())).unwrap();
            }
            drop(complete);
            launch.advance(&mut world);
            let result = receive.try_recv().unwrap();
            if disconnect {
                assert!(result.unwrap_err().contains("disconnected"));
            } else {
                assert_eq!(result.unwrap(), expected);
            }
            assert!(launch.acknowledgement.is_none());
            launch.advance(&mut world);
            assert!(receive.try_recv().is_err());
        }
    }
}
