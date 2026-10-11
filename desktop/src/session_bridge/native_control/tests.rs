use super::*;

struct Fixture {
    root: PathBuf,
    state: SessionBridgeState,
    engine: AppState,
    owner: DocumentContext,
    session: String,
    revision: u64,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("limo-native-admission-{}", Uuid::new_v4()));
        std::env::set_var("LIMO_CAD_SESSION_DIR", &root);
        let state = SessionBridgeState::default();
        let engine = AppState::new();
        state.with_project_session_transition("main", &engine, || {
            engine.bind_project_session("original")
        });
        let session = state.reserve_for_window("main").unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let owner = state.native_document_context("main", &engine).unwrap();
        let revision = state
            .native_document_receipt(&engine, &owner)
            .unwrap()
            .revision;
        fs::create_dir_all(root.join(&session).join("controls")).unwrap();
        Self {
            root,
            state,
            engine,
            owner,
            session,
            revision,
        }
    }

    fn request(&self, id: &str, expiry: u64) -> PathBuf {
        let path = self
            .root
            .join(&self.session)
            .join("controls")
            .join(format!("{id}.request.json"));
        atomic_write(
            &path,
            &json!({"id":id,"expires_ms":expiry,"ui":{"action":"inspect"}}).to_string(),
        )
        .unwrap();
        path
    }

    fn admit(&self, id: &str) -> Ticket {
        self.request(id, now_ms() + 30_000);
        let request = control_for_window_owned(
            &self.state,
            "main",
            &self.engine,
            None,
            Some((&self.owner, id)),
        )
        .unwrap();
        assert!(request["id"] == id && request["session_id"] == self.session);
        ticket_for(&self.state, &self.owner, &self.session, id).unwrap()
    }

    fn response(&self, id: &str) -> Value {
        json!({"request_id":id,"session_id":self.session,"status":"applied","value":{"proof":"actual outcome"}})
    }

    fn read_result(&self, id: &str) -> Value {
        serde_json::from_str(
            &fs::read_to_string(
                self.root
                    .join(&self.session)
                    .join("controls")
                    .join(format!("{id}.result.json")),
            )
            .unwrap(),
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn native_admission_retains_actual_completion_after_expiry_and_unlink_once() {
    super::super::tests::with_isolated_session_test(|| {
        let f = Fixture::new();
        let ticket = f.admit("100-1");
        let marker = marker_path(&ticket);
        let bytes = fs::read(&marker).unwrap();
        assert!(bytes.len() <= limo_cad_mcp::NATIVE_CONTROL_ADMISSION_MAX_BYTES);
        assert!(NativeControlAdmission::decode(std::str::from_utf8(&bytes).unwrap()).is_ok());
        assert!(!f
            .root
            .join(&f.session)
            .join("controls/100-1.request.json")
            .exists());
        validate_start_at(&f.state, &f.engine, &ticket, ticket.marker.expires_ms).unwrap();
        assert!(validate_start_at(&f.state, &f.engine, &ticket, ticket.marker.expires_ms).is_err());
        fs::remove_file(&marker).unwrap();
        let mut response = f.response("100-1");
        complete_at(
            &f.state,
            &f.engine,
            &ticket,
            Some((&f.owner, f.revision)),
            &mut response,
            ticket.marker.expires_ms + 10_000,
        )
        .unwrap();
        let result = f.read_result("100-1");
        assert!(result["status"] == "applied" && result["value"]["proof"] == "actual outcome");
        assert!(result["receipt_published_ms"] == ticket.marker.expires_ms + 10_000);
        assert!(complete(
            &f.state,
            &f.engine,
            &ticket,
            Some((&f.owner, f.revision)),
            &mut response
        )
        .is_err());
        assert!(
            f.read_result("100-1") == result,
            "A replay must not overwrite the receipt"
        );
    });
}

#[test]
fn native_admission_refuses_unadmitted_expired_foreign_and_competing_requests() {
    super::super::tests::with_isolated_session_test(|| {
        let f = Fixture::new();
        assert!(ticket_for(&f.state, &f.owner, &f.session, "200-1").is_err());
        let expired = f.request("200-1", now_ms().saturating_sub(1));
        let mut request: Value =
            serde_json::from_str(&fs::read_to_string(&expired).unwrap()).unwrap();
        request["session_id"] = json!(f.session);
        let mut publishers = f.state.publishers.lock().unwrap();
        assert!(admit(
            publishers.get_mut("main").unwrap(),
            &f.engine,
            &f.state.process_instance_id,
            &f.owner,
            f.revision,
            &expired,
            &request
        )
        .is_err());
        drop(publishers);
        let ticket = f.admit("200-2");
        let mut foreign = f.owner.clone();
        foreign.epoch += 1;
        assert!(ticket_for(&f.state, &foreign, &f.session, "200-2").is_err());
        let mut forged = ticket.clone();
        forged.nonce = Uuid::new_v4();
        assert!(validate_start(&f.state, &f.engine, &forged).is_err());
        f.request("200-3", now_ms() + 30_000);
        assert!(control_for_window_owned(
            &f.state,
            "main",
            &f.engine,
            None,
            Some((&f.owner, "200-3"))
        )
        .is_err());
        assert!(f
            .root
            .join(&f.session)
            .join("controls/200-3.request.json")
            .exists());
        let mut response = f.response("200-2");
        assert!(
            complete(
                &f.state,
                &f.engine,
                &ticket,
                Some((&f.owner, f.revision)),
                &mut response
            )
            .is_err(),
            "An unstarted admission cannot claim application"
        );
        assert!(!f
            .root
            .join(&f.session)
            .join("controls/200-2.result.json")
            .exists());
    });
}

#[test]
fn native_completion_original_ticket_survives_replacement_and_strips_stale_presentation() {
    super::super::tests::with_isolated_session_test(|| {
        let f = Fixture::new();
        let ticket = f.admit("300-1");
        validate_start(&f.state, &f.engine, &ticket).unwrap();
        f.state
            .with_project_session_transition("main", &f.engine, || {
                f.engine.create_project_session("replacement")
            });
        let next = f.state.reserve_for_window("main").unwrap()["session_id"]
            .as_str()
            .unwrap()
            .to_owned();
        f.state.drop_bound_project_session("main", "original");
        let mut response = f.response("300-1");
        response["ui"] = json!({"stale":true});
        complete_at(
            &f.state,
            &f.engine,
            &ticket,
            Some((&f.owner, f.revision)),
            &mut response,
            ticket.marker.expires_ms + 1,
        )
        .unwrap();
        let result = f.read_result("300-1");
        assert!(result["status"] == "failed" && result["operation_status"] == "applied");
        assert!(
            result["operation_may_have_completed"] == true && result["active_session_id"] == next
        );
        assert!(result.get("ui").is_none() && result["value"].is_null());

        let current = f.state.native_document_context("main", &f.engine).unwrap();
        let revision = f
            .state
            .native_document_receipt(&f.engine, &current)
            .unwrap()
            .revision;
        let path = f.root.join(&next).join("controls/300-2.request.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        atomic_write(
            &path,
            &json!({"id":"300-2","expires_ms":now_ms()+30_000,"ui":{"action":"inspect"}})
                .to_string(),
        )
        .unwrap();
        control_for_window_owned(&f.state, "main", &f.engine, None, Some((&current, "300-2")))
            .unwrap();
        let ticket = ticket_for(&f.state, &current, &next, "300-2").unwrap();
        validate_start(&f.state, &f.engine, &ticket).unwrap();
        // A legitimate replacement adopts its produced final presentation owner.
        f.state
            .with_project_session_transition("main", &f.engine, || {
                f.engine.create_project_session("third")
            });
        let final_owner = f.state.native_document_context("main", &f.engine).unwrap();
        let final_revision = f
            .state
            .native_document_receipt(&f.engine, &final_owner)
            .unwrap()
            .revision;
        let mut response = json!({"request_id":"300-2","session_id":next,"status":"applied","value":{"opened":true}});
        complete(
            &f.state,
            &f.engine,
            &ticket,
            Some((&final_owner, final_revision)),
            &mut response,
        )
        .unwrap();
        assert!(response["status"] == "applied" && response["session_id"] == next);
        assert!(revision > 0);
    });
}

#[test]
fn native_owned_query_publishes_exact_read_only_result_and_original_admission() {
    super::super::tests::with_isolated_session_test(|| {
        let f = Fixture::new();
        let id = "600-1";
        let path = f.root.join(&f.session).join("controls/600-1.request.json");
        let expected = parse_engine_envelope(f.engine.engine_call("document", "")).unwrap();
        let before = serde_json::to_string(&f.engine.document_snapshot()).unwrap();
        let geometry_revision = f.engine.geometry_revision();
        let owner = json!({"session_id":f.session,"window_id":"main","document_id":f.owner.document_id,
            "process_instance_id":f.state.process_instance_id,"base_generation":f.revision});
        atomic_write(
            &path,
            &json!({"id":id,"expires_ms":now_ms()+30_000,"owner":owner,
            "sketch_query":{"method":"document","payload":""}})
            .to_string(),
        )
        .unwrap();
        control_for_window_owned(&f.state, "main", &f.engine, None, Some((&f.owner, id))).unwrap();
        assert!(!path.exists());
        let result = f.read_result(id);
        assert!(
            result["status"] == "applied" && result["value"] == expected,
            "The owned query publishes its exact actual read-only value"
        );
        let admission =
            NativeControlAdmission::decode(&result["control_admission"].to_string()).unwrap();
        assert!(
            admission.request_id == id
                && admission.session_id == f.session
                && admission.window_id == f.owner.window_id
                && admission.document_id == f.owner.document_id
                && admission.epoch == f.owner.epoch
                && admission.revision == f.revision
                && admission.process_instance_id == f.state.process_instance_id
        );
        assert!(result["active_session_id"] == f.session);
        assert!(!path.with_file_name("600-1.admitted.json").exists());
        assert!(!has_pending(&f.state, "main").unwrap());
        assert!(ticket_for(&f.state, &f.owner, &f.session, id).is_err());
        assert!(
            control_for_window_owned(&f.state, "main", &f.engine, None, Some((&f.owner, id)))
                .unwrap()
                .is_null()
        );
        assert!(
            f.read_result(id) == result,
            "A consumed query must retain its original result"
        );
        assert!(serde_json::to_string(&f.engine.document_snapshot()).unwrap() == before);
        assert!(f.engine.geometry_revision() == geometry_revision);
    });
}

#[test]
fn native_owned_query_retains_outcome_until_liveness_and_delivery_succeed() {
    super::super::tests::with_isolated_session_test(|| {
        for block_heartbeat in [true, false] {
            let f = Fixture::new();
            let id = "610-1";
            let controls = f.root.join(&f.session).join("controls");
            let request = controls.join(format!("{id}.request.json"));
            let result_path = controls.join(format!("{id}.result.json"));
            let expected = parse_engine_envelope(f.engine.engine_call("document", "")).unwrap();
            let before = serde_json::to_string(&f.engine.document_snapshot()).unwrap();
            let geometry_revision = f.engine.geometry_revision();
            let heartbeat = f.root.join(&f.session).join("heartbeat.json");
            let lease = f
                .root
                .join("_ui/processes")
                .join(format!("{}.json", f.state.process_instance_id));
            f.state.refresh_query_liveness("main").unwrap();
            for path in [&heartbeat, &lease] {
                let mut stale: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                stale["updated_ms"] = json!(now_ms().saturating_sub(120_000));
                atomic_write(path, &stale.to_string()).unwrap();
            }
            atomic_write(
                &request,
                &json!({"id":id,"expires_ms":now_ms()+30_000,
                "owner":{"session_id":f.session,"window_id":"main","document_id":f.owner.document_id,
                    "process_instance_id":f.state.process_instance_id,"base_generation":f.revision},
                "sketch_query":{"method":"document","payload":""}})
                .to_string(),
            )
            .unwrap();
            let blocked = if block_heartbeat { &heartbeat } else { &lease };
            let saved = fs::read(blocked).unwrap();
            fs::remove_file(blocked).unwrap();
            fs::create_dir(blocked).unwrap();
            assert!(control_for_window_owned(
                &f.state,
                "main",
                &f.engine,
                None,
                Some((&f.owner, id))
            )
            .is_err());
            assert!(!result_path.exists());
            assert!(
                !request.exists(),
                "the original request remains claimed, never replayable"
            );
            let ticket = ticket_for(&f.state, &f.owner, &f.session, id).unwrap();
            let retained = {
                let publishers = f.state.publishers.lock().unwrap();
                let admission = publishers["main"].native_control.as_ref().unwrap();
                assert!(admission.started && admission.query_liveness);
                admission.pending_receipt.as_ref().unwrap().0.clone()
            };
            assert_eq!(retained["value"], expected);
            assert!(validate_start(&f.state, &f.engine, &ticket).is_err());
            assert!(retry_pending(&f.state, &f.engine, "main").is_err());
            assert!(!result_path.exists());
            assert!(has_pending(&f.state, "main").unwrap());
            fs::remove_dir(blocked).unwrap();
            atomic_write(blocked, std::str::from_utf8(&saved).unwrap()).unwrap();
            let started = now_ms();
            retry_pending(&f.state, &f.engine, "main").unwrap();
            let result = f.read_result(id);
            assert_eq!(result["status"], "applied");
            assert_eq!(result["value"], expected);
            assert_eq!(result["control_admission"], retained["control_admission"]);
            for path in [&heartbeat, &lease] {
                let fresh: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                assert!(fresh["updated_ms"].as_u64().unwrap() >= started);
            }
            assert!(!has_pending(&f.state, "main").unwrap());
            assert!(!marker_path(&ticket).exists());
            assert!(ticket_for(&f.state, &f.owner, &f.session, id).is_err());
            retry_pending(&f.state, &f.engine, "main").unwrap();
            assert_eq!(f.read_result(id), result, "delivery is one-use");
            assert_eq!(
                serde_json::to_string(&f.engine.document_snapshot()).unwrap(),
                before
            );
            assert_eq!(f.engine.geometry_revision(), geometry_revision);
        }
    });
}

#[test]
fn native_interference_query_releases_live_guards_and_fences_original_completion_once() {
    super::super::tests::with_isolated_session_test(|| {
        for case in [
            "live",
            "clearance",
            "edit",
            "pose",
            "replace",
            "native",
            "liveness",
            "capture_failure",
        ] {
            let f = Fixture::new();
            let mutate = |operation: &str, arguments: Value| {
                f.state
                    .apply_native_mutation(&f.engine, &f.owner, operation, &arguments, || Ok(()))
                    .unwrap()
                    .value
            };
            mutate(
                "sketch_begin",
                json!({"name":"Repeated stock","plane":{"type":"origin_plane","plane":"xy"}}),
            );
            mutate(
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":10.},"ctrl_held":true}),
            );
            mutate("sketch_finish", json!({}));
            mutate(
                "solid_extrude",
                json!({"sketch_name":"Repeated stock","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.}}),
            );
            let body = f.engine.solid_scene_snapshot().bodies[0].id;
            let assembly = f.engine.with_assembly(Clone::clone);
            let definition = assembly
                .component_structure
                .definitions
                .iter()
                .find(|d| d.body_ids.contains(&body))
                .unwrap()
                .id;
            mutate(
                "assembly_create_occurrence",
                json!({"component_id":definition,"name":"Repeated native body",
                "local_pose":{"translation":[if case=="clearance" {12.}else{5.},0.,0.],"rotation":[0.,0.,0.,1.]}}),
            );
            let repeated = f.engine.with_assembly(|assembly| {
                assembly
                    .component_structure
                    .occurrences
                    .iter()
                    .find(|o| o.name == "Repeated native body")
                    .unwrap()
                    .id
            });
            let revision = f
                .state
                .native_document_receipt(&f.engine, &f.owner)
                .unwrap()
                .revision;
            let before =
                parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
            let id = "640-1";
            let controls = f.root.join(&f.session).join("controls");
            let request_path = controls.join(format!("{id}.request.json"));
            let result_path = controls.join(format!("{id}.result.json"));
            let payload = json!({"occurrence_ids":[],"clearance_threshold_mm":if case=="capture_failure" {-1.}else{3.}})
            .to_string();
            atomic_write(&request_path, &json!({"id":id,"expires_ms":now_ms()+30_000,
                "owner":{"session_id":f.session,"window_id":"main","document_id":f.owner.document_id,
                    "process_instance_id":f.state.process_instance_id,"base_generation":revision},
                "sketch_query":{"method":"assembly_interference_check","payload":payload}}).to_string()).unwrap();
            f.state.refresh_query_liveness("main").unwrap();
            let heartbeat = f.root.join(&f.session).join("heartbeat.json");
            let saved_heartbeat = fs::read(&heartbeat).unwrap();
            let mut observed = 0;
            let mut original_ticket = None;
            let outcome = control_for_window_owned_with_detached_observers(
                &f.state,
                "main",
                &f.engine,
                None,
                Some((&f.owner, id)),
                |_, _| panic!("Interference must not emit drawing projections"),
                (
                    |_| panic!("Interference must not compute a section"),
                    |finished| {
                        observed += 1;
                        assert_eq!(observed, 1);
                        let report = finished.report();
                        assert!(report.exact);
                        assert_eq!(report.pairs.len(), 1);
                        let pair = &report.pairs[0];
                        assert_eq!(pair.body_a, pair.body_b);
                        assert_ne!(pair.occurrence_a, pair.occurrence_b);
                        if case == "clearance" {
                            assert!(!pair.interfering && pair.below_clearance);
                            assert!((pair.minimum_clearance_mm - 2.).abs() < 1e-6);
                        } else {
                            assert!(
                                pair.interfering && (pair.overlap_volume_mm3 - 500.).abs() < 1e-6
                            );
                        }
                        let lease = f.state.process_lease_path.try_lock().unwrap();
                        let publishers = f.state.publishers.try_lock().unwrap();
                        let admission = publishers["main"].native_control.as_ref().unwrap();
                        assert!(admission.started && admission.pending_receipt.is_none());
                        original_ticket = Some(admission.ticket.clone());
                        f.engine.with_drawing(|_| ());
                        drop(publishers);
                        drop(lease);
                        match case {
                            "edit" => {
                                mutate(
                                    "cad_set_document_name",
                                    json!({"name":"Edited while interference computes"}),
                                );
                            }
                            "replace" => {
                                mutate("cad_new_project", json!({}));
                            }
                            "pose" => {
                                parse_engine_envelope(f.engine.engine_call("assembly_update_occurrence",
                            &json!({"occurrence":{"id":repeated,"local_pose":{"translation":[20.,0.,0.],"rotation":[0.,0.,0.,1.]}}}).to_string())).unwrap();
                            }
                            "native" => {
                                parse_engine_envelope(f.engine.project_load(
                                    &serde_json::to_string(before.as_str().unwrap()).unwrap(),
                                ))
                                .unwrap();
                            }
                            "liveness" => {
                                fs::remove_file(&heartbeat).unwrap();
                                fs::create_dir(&heartbeat).unwrap();
                            }
                            _ => {}
                        }
                    },
                ),
            );
            assert!(!request_path.exists());
            if case == "liveness" {
                assert!(outcome.is_err() && !result_path.exists());
                let publishers = f.state.publishers.lock().unwrap();
                let admission = publishers["main"].native_control.as_ref().unwrap();
                assert_eq!(Some(&admission.ticket), original_ticket.as_ref());
                assert!(
                    admission.started
                        && admission.query_liveness
                        && admission.pending_receipt.is_some()
                );
                drop(publishers);
                fs::remove_dir(&heartbeat).unwrap();
                atomic_write(&heartbeat, std::str::from_utf8(&saved_heartbeat).unwrap()).unwrap();
                retry_pending(&f.state, &f.engine, "main").unwrap();
                assert_eq!(observed, 1, "Delivery retry must not inspect again");
            } else {
                assert!(
                    outcome.unwrap().is_null(),
                    "Interference response grants no drawing handoff"
                );
            }
            let response = f.read_result(id);
            let delivered =
                NativeControlAdmission::decode(&response["control_admission"].to_string()).unwrap();
            assert_eq!(delivered.document_id, f.owner.document_id);
            assert_eq!(delivered.epoch, f.owner.epoch);
            assert_eq!(delivered.revision, revision);
            if let Some(ticket) = original_ticket {
                assert_eq!(delivered, ticket.marker);
            }
            if matches!(case, "live" | "clearance" | "liveness") {
                assert_eq!(response["status"], "applied");
                assert_eq!(response["value"]["exact"], true);
                assert_eq!(response["value"]["pairs"].as_array().unwrap().len(), 1);
            } else {
                assert_eq!(response["status"], "failed");
                assert!(response["value"].is_null());
            }
            if matches!(
                case,
                "live" | "clearance" | "native" | "liveness" | "capture_failure"
            ) {
                assert!(
                    parse_engine_envelope(f.engine.engine_call("project_export_model", ""))
                        .unwrap()
                        == before,
                    "Interference query must preserve the complete authored model"
                );
            }
            assert_eq!(observed, usize::from(case != "capture_failure"));
            assert!(!has_pending(&f.state, "main").unwrap());
            assert!(!controls.join(format!("{id}.admitted.json")).exists());
            let original = f.read_result(id);
            retry_pending(&f.state, &f.engine, "main").unwrap();
            assert!(f.read_result(id) == original, "Completion is one-use");
        }
    });
}

#[test]
fn native_section_query_releases_live_guards_and_fences_original_completion_once() {
    super::super::tests::with_isolated_session_test(|| {
        for case in [
            "live",
            "edit",
            "replace",
            "native",
            "liveness",
            "capture_failure",
        ] {
            let f = Fixture::new();
            let mutate = |operation: &str, arguments: Value| {
                f.state
                    .apply_native_mutation(&f.engine, &f.owner, operation, &arguments, || Ok(()))
                    .unwrap()
                    .value
            };
            mutate(
                "sketch_begin",
                json!({"name":"Section stock","plane":{"type":"origin_plane","plane":"xy"}}),
            );
            mutate(
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":18.,"y":12.},"ctrl_held":true}),
            );
            mutate("sketch_finish", json!({}));
            mutate(
                "solid_extrude",
                json!({"sketch_name":"Section stock","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":8.}}),
            );
            let body = f.engine.solid_scene_snapshot().bodies[0].id;
            let revision = f
                .state
                .native_document_receipt(&f.engine, &f.owner)
                .unwrap()
                .revision;
            let before =
                parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
            let id = "630-1";
            let controls = f.root.join(&f.session).join("controls");
            let request_path = controls.join(format!("{id}.request.json"));
            let result_path = controls.join(format!("{id}.result.json"));
            let payload = json!({"body_id":if case=="capture_failure" {9999} else {body.0},
                "plane":"xy","offset_mm":4.,"probe_mm":6.,"include_cutaway":true})
            .to_string();
            atomic_write(&request_path, &json!({"id":id,"expires_ms":now_ms()+30_000,
                "owner":{"session_id":f.session,"window_id":"main","document_id":f.owner.document_id,
                    "process_instance_id":f.state.process_instance_id,"base_generation":revision},
                "sketch_query":{"method":"solid_section_review","payload":payload}}).to_string()).unwrap();
            f.state.refresh_query_liveness("main").unwrap();
            let heartbeat = f.root.join(&f.session).join("heartbeat.json");
            let saved_heartbeat = fs::read(&heartbeat).unwrap();
            let mut observed = 0;
            let mut original_ticket = None;
            let outcome = control_for_window_owned_with_observers(
                &f.state,
                "main",
                &f.engine,
                None,
                Some((&f.owner, id)),
                |_, _| panic!("Section must not emit drawing projections"),
                |finished| {
                    observed += 1;
                    assert_eq!(observed, 1);
                    assert_eq!(finished.report().bounds_mm.unwrap(), [0., 0., 18., 12.]);
                    assert!(finished.report().cutaway.is_some());
                    let lease = f.state.process_lease_path.try_lock().unwrap();
                    let publishers = f.state.publishers.try_lock().unwrap();
                    let admission = publishers["main"].native_control.as_ref().unwrap();
                    assert!(admission.started && admission.pending_receipt.is_none());
                    original_ticket = Some(admission.ticket.clone());
                    f.engine.with_drawing(|_| ());
                    drop(publishers);
                    drop(lease);
                    match case {
                        "edit" => {
                            mutate(
                                "cad_set_document_name",
                                json!({"name":"Edited while section computes"}),
                            );
                        }
                        "replace" => {
                            mutate("cad_new_project", json!({}));
                        }
                        "native" => {
                            parse_engine_envelope(f.engine.project_load(
                                &serde_json::to_string(before.as_str().unwrap()).unwrap(),
                            ))
                            .unwrap();
                        }
                        "liveness" => {
                            fs::remove_file(&heartbeat).unwrap();
                            fs::create_dir(&heartbeat).unwrap();
                        }
                        _ => {}
                    }
                },
            );
            assert!(!request_path.exists());
            if case == "liveness" {
                assert!(outcome.is_err() && !result_path.exists());
                let publishers = f.state.publishers.lock().unwrap();
                let admission = publishers["main"].native_control.as_ref().unwrap();
                assert_eq!(Some(&admission.ticket), original_ticket.as_ref());
                assert!(
                    admission.started
                        && admission.query_liveness
                        && admission.pending_receipt.is_some()
                );
                drop(publishers);
                fs::remove_dir(&heartbeat).unwrap();
                atomic_write(&heartbeat, std::str::from_utf8(&saved_heartbeat).unwrap()).unwrap();
                retry_pending(&f.state, &f.engine, "main").unwrap();
                assert_eq!(observed, 1, "Delivery retry must not inspect again");
            } else {
                assert!(
                    outcome.unwrap().is_null(),
                    "Section response grants no drawing handoff"
                );
            }
            let response = f.read_result(id);
            let delivered =
                NativeControlAdmission::decode(&response["control_admission"].to_string()).unwrap();
            assert_eq!(delivered.document_id, f.owner.document_id);
            assert_eq!(delivered.epoch, f.owner.epoch);
            assert_eq!(delivered.revision, revision);
            if let Some(ticket) = original_ticket {
                assert_eq!(delivered, ticket.marker);
            }
            if matches!(case, "live" | "liveness") {
                assert_eq!(response["status"], "applied");
                assert_eq!(response["value"]["outcome"], "material_section");
                assert_eq!(response["value"]["bounds_mm"], json!([0., 0., 18., 12.]));
                assert!(response["value"]["cutaway"].is_object());
            } else {
                assert_eq!(response["status"], "failed");
                assert!(response["value"].is_null());
            }
            if matches!(case, "live" | "native" | "liveness" | "capture_failure") {
                assert!(
                    parse_engine_envelope(f.engine.engine_call("project_export_model", ""))
                        .unwrap()
                        == before,
                    "Section query must preserve the complete authored model"
                );
            }
            assert_eq!(observed, usize::from(case != "capture_failure"));
            assert!(!has_pending(&f.state, "main").unwrap());
            assert!(!controls.join(format!("{id}.admitted.json")).exists());
            let original = f.read_result(id);
            retry_pending(&f.state, &f.engine, "main").unwrap();
            assert!(f.read_result(id) == original, "Completion is one-use");
        }
    });
}

#[test]
fn native_drawing_query_releases_live_guards_and_fences_original_completion_once() {
    super::super::tests::with_isolated_session_test(|| {
        for case in ["live", "edit", "replace", "liveness", "capture_failure"] {
            let f = Fixture::new();
            let mutate = |operation: &str, arguments: Value| {
                f.state
                    .apply_native_mutation(&f.engine, &f.owner, operation, &arguments, || Ok(()))
                    .unwrap()
                    .value
            };
            mutate(
                "sketch_begin",
                json!({"name":"Query box","plane":{"type":"origin_plane","plane":"xy"}}),
            );
            mutate(
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true}),
            );
            mutate("sketch_finish", json!({}));
            mutate(
                "solid_extrude",
                json!({"sketch_name":"Query box","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":6.}}),
            );
            let scene = f.engine.solid_scene_snapshot();
            assert_eq!(scene.bodies.len(), 1);
            let body = scene.bodies[0].id;
            mutate(
                "drawing_create_sheet",
                json!({"name":"Native query","format":"a4","orientation":"landscape"}),
            );
            mutate(
                "drawing_add_view",
                json!({"sheet_id":1,"view":{"name":"Top","kind":"top","body_ids":[body],"direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[100.,75.],"scale":1.}}),
            );
            let revision = f
                .state
                .native_document_receipt(&f.engine, &f.owner)
                .unwrap()
                .revision;
            let before =
                parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
            let id = "620-1";
            let controls = f.root.join(&f.session).join("controls");
            let request_path = controls.join(format!("{id}.request.json"));
            let result_path = controls.join(format!("{id}.result.json"));
            let payload =
                json!({"sheet_id":if case == "capture_failure" {999} else {1},"format":"svg"})
                    .to_string();
            atomic_write(&request_path, &json!({"id":id,"expires_ms":now_ms()+30_000,
                "owner":{"session_id":f.session,"window_id":"main","document_id":f.owner.document_id,"process_instance_id":f.state.process_instance_id,"base_generation":revision},
                "sketch_query":{"method":"drawing_export","payload":payload}}).to_string()).unwrap();
            f.state.refresh_query_liveness("main").unwrap();
            let heartbeat = f.root.join(&f.session).join("heartbeat.json");
            let saved_heartbeat = fs::read(&heartbeat).unwrap();
            let mut observed = 0;
            let mut original_ticket = None;
            let mut actual_projection = None;
            let outcome = control_for_window_owned_observing(
                &f.state,
                "main",
                &f.engine,
                None,
                Some((&f.owner, id)),
                |request, projection| {
                    observed += 1;
                    assert_eq!(observed, 1);
                    assert!(!projection.visible.is_empty());
                    actual_projection = Some(json!({"request":request,"projection":projection}));
                    // These are the actual production locks while the real
                    // exporter invokes its actual completed-projection observer.
                    let _lease = f.state.process_lease_path.try_lock().unwrap();
                    let publishers = f.state.publishers.try_lock().unwrap();
                    let admission = publishers["main"].native_control.as_ref().unwrap();
                    assert!(admission.started && admission.pending_receipt.is_none());
                    original_ticket = Some(admission.ticket.clone());
                    f.engine
                        .with_drawing(|drawing| assert_eq!(drawing.sheets.len(), 1));
                    drop(publishers);
                    drop(_lease);
                    match case {
                        "edit" => {
                            let mut drawing = (*f.engine.shared_drawing_snapshot()).clone();
                            drawing.sheets[0].name = "Changed during native query".into();
                            mutate(
                                "drawing_set_document",
                                serde_json::to_value(drawing).unwrap(),
                            );
                        }
                        "replace" => {
                            mutate("cad_new_project", json!({}));
                        }
                        "liveness" => {
                            fs::remove_file(&heartbeat).unwrap();
                            fs::create_dir(&heartbeat).unwrap();
                        }
                        _ => {}
                    }
                },
            );
            assert!(!request_path.exists());
            if case == "liveness" {
                assert!(outcome.is_err() && !result_path.exists());
                let publishers = f.state.publishers.lock().unwrap();
                let admission = publishers["main"].native_control.as_ref().unwrap();
                assert_eq!(Some(&admission.ticket), original_ticket.as_ref());
                assert!(
                    admission.started
                        && admission.pending_receipt.is_some()
                        && admission.query_liveness
                );
                drop(publishers);
                fs::remove_dir(&heartbeat).unwrap();
                atomic_write(&heartbeat, std::str::from_utf8(&saved_heartbeat).unwrap()).unwrap();
                retry_pending(&f.state, &f.engine, "main").unwrap();
                assert_eq!(
                    observed, 1,
                    "Delivery retry must not export or observe again"
                );
            } else {
                let handoff = outcome.unwrap();
                if case == "live" {
                    assert_eq!(handoff["document_id"], f.owner.document_id);
                    assert_eq!(handoff["engine_revision"], revision);
                    assert_eq!(
                        handoff["drawing_projections"][0],
                        actual_projection.unwrap()
                    );
                } else {
                    assert!(
                        handoff.is_null(),
                        "Stale or failed geometry cannot enter presentation"
                    );
                }
            }
            let response = f.read_result(id);
            let delivered =
                NativeControlAdmission::decode(&response["control_admission"].to_string()).unwrap();
            assert_eq!(delivered.document_id, f.owner.document_id);
            assert_eq!(delivered.epoch, f.owner.epoch);
            assert_eq!(delivered.revision, revision);
            if let Some(ticket) = original_ticket {
                assert_eq!(delivered, ticket.marker);
            }
            if matches!(case, "live" | "liveness") {
                assert_eq!(response["status"], "applied");
                assert!(!response["value"]["content"].as_str().unwrap().is_empty());
                assert_eq!(
                    parse_engine_envelope(f.engine.engine_call("project_export_model", ""))
                        .unwrap(),
                    before
                );
            } else {
                assert_eq!(response["status"], "failed");
                assert!(response["value"].is_null());
                if case != "capture_failure" {
                    assert_eq!(response["render_status"], "owner_changed");
                }
            }
            assert_eq!(observed, usize::from(case != "capture_failure"));
            assert!(!has_pending(&f.state, "main").unwrap());
            assert!(!controls.join(format!("{id}.admitted.json")).exists());
        }
    });
}

#[test]
fn native_rejected_completion_retries_transport_without_restarting_action() {
    super::super::tests::with_isolated_session_test(|| {
        let f = Fixture::new();
        let ticket = f.admit("400-1");
        let result_path = marker_path(&ticket).with_file_name("400-1.result.json");
        fs::create_dir(&result_path).unwrap();
        let mut response = json!({"request_id":"400-1","session_id":f.session,"status":"failed","mutation_applied":false,"error":"rejected before dispatch"});
        assert!(matches!(
            complete(&f.state, &f.engine, &ticket, None, &mut response),
            Err(CompletionError::Publication(_))
        ));
        assert!(f.state.publishers.lock().unwrap()["main"]
            .native_control
            .as_ref()
            .unwrap()
            .pending_receipt
            .is_some());
        assert!(
            validate_start(&f.state, &f.engine, &ticket).is_err(),
            "A produced rejection awaiting transport cannot start an action"
        );
        let mut competing = response.clone();
        competing["error"] = json!("different outcome");
        assert!(matches!(
            complete(&f.state, &f.engine, &ticket, None, &mut competing),
            Err(CompletionError::Rejected(_))
        ));
        fs::remove_dir(&result_path).unwrap();
        retry_pending(&f.state, &f.engine, "main").unwrap();
        let result = f.read_result("400-1");
        assert!(
            result["error"] == "rejected before dispatch" && result["mutation_applied"] == false
        );
        assert!(f.state.publishers.lock().unwrap()["main"]
            .native_control
            .is_none());

        // Claim and start first; only then create a publication collision. A
        // pre-existing result path correctly prevents queue admission entirely.
        let ticket = f.admit("400-2");
        validate_start(&f.state, &f.engine, &ticket).unwrap();
        let produced = parse_engine_envelope(f.engine.engine_call("document", "")).unwrap();
        let before = serde_json::to_string(&f.engine.document_snapshot()).unwrap();
        let mut response = json!({"request_id":"400-2","session_id":f.session,
            "status":"applied","value":produced});
        let result_path = marker_path(&ticket).with_file_name("400-2.result.json");
        fs::create_dir(&result_path).unwrap();
        assert!(matches!(
            complete(
                &f.state,
                &f.engine,
                &ticket,
                Some((&f.owner, f.revision)),
                &mut response
            ),
            Err(CompletionError::Publication(_))
        ));
        let retained = f.state.publishers.lock().unwrap()["main"]
            .native_control
            .as_ref()
            .unwrap()
            .pending_receipt
            .as_ref()
            .unwrap()
            .0
            .clone();
        assert!(retained["value"] == produced && retained["status"] == "applied");
        assert!(
            validate_start(&f.state, &f.engine, &ticket).is_err(),
            "Retained transport proof never authorizes a second dispatch"
        );
        let mut competing = retained.clone();
        competing["value"] = json!({"different":"outcome"});
        assert!(matches!(
            complete(
                &f.state,
                &f.engine,
                &ticket,
                Some((&f.owner, f.revision)),
                &mut competing
            ),
            Err(CompletionError::Rejected(_))
        ));
        fs::remove_dir(&result_path).unwrap();
        retry_pending(&f.state, &f.engine, "main").unwrap();
        let result = f.read_result("400-2");
        assert!(
            result["status"] == "applied" && result["value"] == produced,
            "Retry publishes the exact previously produced query outcome"
        );
        assert!(serde_json::to_string(&f.engine.document_snapshot()).unwrap() == before);
        assert!(!has_pending(&f.state, "main").unwrap());
        assert!(!marker_path(&ticket).exists());
        assert!(!f
            .root
            .join(&f.session)
            .join("controls/400-2.request.json")
            .exists());
    });
}
