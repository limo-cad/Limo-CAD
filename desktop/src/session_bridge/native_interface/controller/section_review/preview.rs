//! One disposable read worker. Only the newest draft is dispatched after an
//! in-flight query; it never occupies the native document mutation worker.
use super::*;
use limo_cad_native_engine::{FinishedSectionReview, PreparedSectionReview};
use std::sync::mpsc;

type SourceKey = (DocumentContext, u64, BodyId);
type PreparedPreview = (
    FinishedSectionReview,
    Option<Image>,
    Option<Arc<native_viewport::section_view::PreparedCutaway>>,
);
struct Job {
    key: SourceKey,
    generation: u64,
    source: Option<PreparedSectionReview>,
    request: SectionReviewRequest,
}
struct Completed {
    key: SourceKey,
    generation: u64,
    result: Result<PreparedPreview, String>,
}
pub(super) struct Worker {
    jobs: mpsc::SyncSender<Job>,
    completed: std::sync::Mutex<mpsc::Receiver<Completed>>,
    key: Option<SourceKey>,
    running: bool,
}
impl Worker {
    fn new(handle: NativeInterfaceHandle) -> Result<Self, String> {
        let (jobs, incoming) = mpsc::sync_channel::<Job>(1);
        let (output, completed) = mpsc::channel();
        std::thread::Builder::new()
            .name("cad-section-preview".into())
            .spawn(move || {
                let mut source = None;
                while let Ok(job) = incoming.recv() {
                    if job.source.is_some() {
                        source = job.source;
                    }
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut finished = source
                            .as_ref()
                            .ok_or("Section source was released")?
                            .inspect_at(&job.request)?;
                        let report = finished.report_mut();
                        let image = if job.request.include_cutaway || report.svg.is_empty() {
                            None
                        } else {
                            Some(rasterize(&report.svg)?)
                        };
                        let cutaway = report.cutaway.take().map(|body| {
                            Arc::new(native_viewport::section_view::PreparedCutaway::new(body))
                        });
                        Ok((finished, image, cutaway))
                    }))
                    .unwrap_or_else(|_| {
                        Err("Section preview failed; adjust the plane to retry".into())
                    });
                    if output
                        .send(Completed {
                            key: job.key,
                            generation: job.generation,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                    handle.request_redraw();
                }
            })
            .map_err(|error| format!("Cannot start section preview: {error}"))?;
        Ok(Self {
            jobs,
            completed: std::sync::Mutex::new(completed),
            key: None,
            running: false,
        })
    }
}

fn adopt(
    state: &mut State,
    services: &NativeServices,
    completed: Completed,
) -> Result<bool, String> {
    if !state.current(&completed.key.0, completed.key.1, completed.generation)
        && !completed.result.as_ref().is_ok_and(|(finished, _, _)| {
            state.accepts_preview(
                &completed.key.0,
                completed.key.1,
                &finished.report().request,
            )
        })
    {
        return Ok(false);
    }
    services
        .bridge
        .with_native_document_receipt(&services.engine, &completed.key.0, |current| {
            if current != completed.key.1 {
                return Err("The section source model changed".into());
            }
            let (finished, image, cutaway) = completed.result?;
            services.engine.validate_section_review(&finished)?;
            let report = finished.into_report();
            // Intermediate positions keep a sustained drag moving. Only the
            // completed current draft enables export.
            state.finished_generation = Some(completed.generation);
            state.message = match report.outcome {
                SectionOutcome::NoIntersection => "Plane does not intersect this body.".into(),
                SectionOutcome::BoundaryContact => {
                    "Plane touches the boundary. Move inside the body to reveal a section.".into()
                }
                SectionOutcome::MaterialSection if report.request.probe_mm.is_some() => {
                    if report.probe_spans.is_empty() {
                        "No material at this probe coordinate.".into()
                    } else {
                        report
                            .probe_spans
                            .iter()
                            .map(|span| format!("{:.3} mm", span.length_mm))
                            .collect::<Vec<_>>()
                            .join(" · ")
                    }
                }
                SectionOutcome::MaterialSection if report.request.include_cutaway => {
                    "Drag the bar to move the cut. Orange faces show the section.".into()
                }
                SectionOutcome::MaterialSection => {
                    "Drag the bar to move the section. Copy SVG for the full diagram.".into()
                }
            };
            state.cutaway = cutaway;
            state.report = Some(report);
            state.prepared_image = image;
            state.image = None;
            Ok(true)
        })
}

pub(super) fn synchronize(
    world: &World,
    state: &mut State,
    services: &NativeServices,
    owner: &DocumentContext,
) -> Result<(), String> {
    let completed =
        state
            .preview
            .as_mut()
            .and_then(|worker| match worker.completed.lock().ok()?.try_recv() {
                Ok(completed) => {
                    worker.running = false;
                    Some(completed)
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    worker.running = false;
                    None
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
    if let Some(completed) = completed {
        adopt(state, services, completed)?;
    }
    if state.finished_generation == Some(state.generation)
        || state.preview.as_ref().is_some_and(|worker| worker.running)
        || worker::busy(world)
    {
        return Ok(());
    }
    let request = match state.request(services.engine.document_units()) {
        Ok(request) => request,
        Err(error) => {
            state.finished_generation = Some(state.generation);
            state.report = None;
            state.image = None;
            state.cutaway = None;
            state.message = error;
            return Ok(());
        }
    };
    if state.preview.is_none() {
        state.preview = Some(Worker::new(
            world.resource::<NativeInterfaceHandle>().clone(),
        )?);
    }
    let key = (owner.clone(), state.revision, request.body_id);
    let preview = state.preview.as_mut().unwrap();
    let source = if preview.key.as_ref() != Some(&key) {
        let captured =
            services
                .bridge
                .with_native_document_receipt(&services.engine, owner, |current| {
                    if current != state.revision {
                        return Err("The section source model changed".into());
                    }
                    services.engine.prepare_section_review(
                        &serde_json::to_string(&request).map_err(|error| error.to_string())?,
                    )
                });
        match captured {
            Ok(source) => Some(source),
            Err(error) => {
                state.finished_generation = Some(state.generation);
                state.message = error;
                return Ok(());
            }
        }
    } else {
        None
    };
    preview
        .jobs
        .try_send(Job {
            key: key.clone(),
            generation: state.generation,
            source,
            request,
        })
        .map_err(|error| format!("Cannot update section preview: {error}"))?;
    preview.key = Some(key);
    preview.running = true;
    state.message = "Updating section…".into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // Carries forward PR124's source-replacement cases through the live
    // preview path, including native changes that do not alter the UI receipt.
    #[test]
    fn live_section_adoption_keeps_guards_available_and_rejects_stale_sources() {
        use crate::session_bridge::native_interface::tests::Fixture;
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        for case in [
            "live", "drag", "side", "close", "edit", "replace", "native", "owner",
        ] {
            let fixture = Fixture::new();
            let owner = fixture.owner();
            for (operation, arguments) in [
                (
                    "sketch_begin",
                    json!({"name":"Section stock","plane":{"type":"origin_plane","plane":"xy"}}),
                ),
                (
                    "sketch_add_rectangle",
                    json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":18.,"y":12.},"ctrl_held":true}),
                ),
                ("sketch_finish", json!({})),
                (
                    "solid_extrude",
                    json!({"sketch_name":"Section stock","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":8.}}),
                ),
            ] {
                fixture
                    .bridge
                    .apply_native_mutation(
                        &fixture.engine,
                        &owner,
                        operation,
                        &arguments,
                        || Ok(()),
                    )
                    .unwrap();
            }
            let body = fixture.engine.solid_scene_snapshot().bodies[0].id;
            let before = fixture.engine.engine_call("project_export_model", "");
            let receipt = fixture
                .bridge
                .native_document_receipt(&fixture.engine, &owner)
                .unwrap();
            let services = NativeServices {
                engine: fixture.engine.clone(),
                bridge: fixture.bridge.clone(),
            };
            let mut state = State {
                owner: Some(owner.clone()),
                revision: receipt.revision,
                visible: true,
                in_3d: true,
                body: body.0.to_string(),
                bodies: vec![(body.0, "Stock".into())],
                plane: "xy".into(),
                offset: "4".into(),
                side: "negative".into(),
                ..Default::default()
            };
            let request = state.request(UnitSystem::Mm).unwrap();
            let source = services
                .bridge
                .with_native_document_receipt(&services.engine, &owner, |_| {
                    services
                        .engine
                        .prepare_section_review(&serde_json::to_string(&request).unwrap())
                })
                .unwrap();
            let mut finished = std::thread::spawn(move || source.inspect_at(&request).unwrap())
                .join()
                .unwrap();
            services
                .bridge
                .with_native_document_receipt(&services.engine, &owner, |_| {
                    services.engine.with_drawing(|_| ());
                    Ok(())
                })
                .unwrap();
            assert_eq!(finished.report().bounds_mm.unwrap(), [0., 0., 18., 12.]);
            let cutaway =
                finished.report_mut().cutaway.take().map(|body| {
                    Arc::new(native_viewport::section_view::PreparedCutaway::new(body))
                });
            match case {
                "drag" => state.set_value(Field::Offset, "3".into()),
                "side" => state.set_value(Field::Side, "positive".into()),
                "close" => {
                    state.visible = false;
                    state.reset_controls("");
                }
                "owner" => state.owner.as_mut().unwrap().epoch += 1,
                "edit" => {
                    fixture
                        .rename(&owner, "Changed during section preview")
                        .unwrap();
                }
                "replace" => {
                    fixture
                        .bridge
                        .apply_native_mutation(
                            &fixture.engine,
                            &owner,
                            "cad_new_project",
                            &json!({}),
                            || Ok(()),
                        )
                        .unwrap();
                }
                "native" => {
                    let model =
                        crate::session_bridge::parse_engine_envelope(before.clone()).unwrap();
                    crate::session_bridge::parse_engine_envelope(
                        fixture
                            .engine
                            .project_load(&serde_json::to_string(model.as_str().unwrap()).unwrap()),
                    )
                    .unwrap();
                }
                _ => {}
            }
            let result = adopt(
                &mut state,
                &services,
                Completed {
                    key: (owner.clone(), receipt.revision, body),
                    generation: 0,
                    result: Ok((finished, None, cutaway)),
                },
            );
            if matches!(case, "live" | "drag") {
                assert!(result.unwrap());
                assert!(state.report.is_some() && state.cutaway.is_some());
                assert_eq!(state.report_current(), case == "live");
            } else {
                assert!(result.is_err() || result == Ok(false));
                assert!(
                    state.report.is_none()
                        && state.cutaway.is_none()
                        && state.prepared_image.is_none()
                );
            }
            if matches!(
                case,
                "live" | "drag" | "side" | "close" | "owner" | "native"
            ) {
                assert_eq!(
                    fixture.engine.engine_call("project_export_model", ""),
                    before
                );
            }
        }
    }
    #[test]
    fn rapid_slider_changes_finish_at_latest_position_without_blocking_native_inputs_or_editing_model(
    ) {
        use crate::session_bridge::native_interface::tests::Fixture;
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let checked = |raw| crate::session_bridge::parse_engine_envelope(raw).unwrap();
        checked(
            fixture
                .engine
                .engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#),
        );
        checked(fixture.engine.engine_call("add_rectangle", &json!({"mode":"two_point","p1":{"x":-10.,"y":-6.},"p2":{"x":10.,"y":6.},"ctrl_held":true}).to_string()));
        checked(fixture.engine.engine_call("end_sketch", ""));
        checked(fixture.engine.solid_extrude(&json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.}}).to_string()));
        let scene = fixture.engine.solid_scene_snapshot();
        let body = scene.bodies[0].id;
        let owner = fixture.owner();
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        let before = fixture.engine.engine_call("project_export_model", "");
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let mut world = World::new();
        world.insert_resource(NativeInterfaceHandle::new(|| {}));
        let mut state = State {
            owner: Some(owner.clone()),
            revision: receipt.revision,
            visible: true,
            in_3d: true,
            body: body.0.to_string(),
            bodies: vec![(body.0, "Fixture".into())],
            plane: "xy".into(),
            offset: "2".into(),
            side: "negative".into(),
            ..Default::default()
        };
        state.source(&scene, false);
        synchronize(&world, &mut state, &services, &owner).unwrap();
        assert!(state.preview.as_ref().unwrap().running);
        assert!(!worker::busy(&world));
        for value in [4., 6., 8.] {
            state.set_position(value).unwrap();
            synchronize(&world, &mut state, &services, &owner).unwrap();
            assert!(!worker::busy(&world));
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !state.report_current() {
            synchronize(&world, &mut state, &services, &owner).unwrap();
            assert!(
                std::time::Instant::now() < deadline,
                "Live section did not settle"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let report = state.report.as_ref().unwrap();
        assert_eq!(report.request.offset_mm, 8.);
        assert_eq!(report.outcome, SectionOutcome::MaterialSection);
        assert!(state.cutaway.is_some());
        assert_eq!(
            fixture.engine.engine_call("project_export_model", ""),
            before
        );
        assert!(Arc::ptr_eq(&scene, &fixture.engine.solid_scene_snapshot()));
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &owner)
                .unwrap(),
            receipt
        );
        state.set_position(5.).unwrap();
        synchronize(&world, &mut state, &services, &owner).unwrap();
        world.insert_resource(state);
        assert!(escape(&mut world));
        let closed = world.resource::<State>();
        assert!(closed.preview.is_none());
        assert!(closed.report.is_none());
        assert!(closed.cutaway.is_none());
    }
}
