use super::*;
use std::cell::Cell;

fn limits() -> EvaluationLimits {
    EvaluationLimits {
        max_bytes: 64 * 1024,
        max_nodes: 512,
        max_depth: 32,
        max_work: 10_000,
    }
}
fn generous() -> EvaluationLimits {
    EvaluationLimits {
        max_bytes: 512 * 1024,
        max_nodes: 4096,
        ..limits()
    }
}
fn no_call(_: &str, _: Value, _: RunProgress) -> Result<Value, String> {
    panic!("data-only script must not call the host")
}
fn script(steps: Value, exports: Value) -> Script {
    Script::parse(
        &json!({"version":1,"name":"Small budget","steps":steps,"exports":exports}).to_string(),
    )
    .unwrap()
}

#[test]
fn repeated_references_reject_in_actual_interpreter_at_small_budget() {
    // Same shape as the review witness; NEVER run this using an unbounded API.
    let mut steps = vec![json!({"let":{"b0":"a".repeat(1024)}})];
    for i in 1..=21 {
        let previous = format!("b{}", i - 1);
        let mut binding = Map::new();
        binding.insert(
            format!("b{i}"),
            json!([{"$ref":previous},{"$ref":previous}]),
        );
        steps.push(json!({"let":binding}));
    }
    let script = script(json!(steps), json!({"value":{"$ref":"b21"}}));
    let error =
        run_bounded(&script, no_call, RunOptions::default(), limits(), || Ok(())).unwrap_err();
    assert!(error.contains("evaluation byte limit"), "{error}");
    assert!(
        error.contains("stopped at step"),
        "must reach bounded resolution: {error}"
    );
    assert!(!error.contains("22 completed"));
}

#[test]
fn clone_is_charged_before_string_allocation_and_failures_do_not_grow_budget() {
    let value = json!("a".repeat(32));
    let mut host = CallbackHost(no_call, || Ok(()));
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_bytes: 200,
            ..limits()
        }),
    )
    .unwrap();
    assert_eq!(evaluation.cloned(&value, 0).unwrap(), value);
    assert!(evaluation
        .cloned(&value, 0)
        .unwrap_err()
        .contains("byte limit"));
    // A fresh run can retry; reservations do not live on the Script or host.
    let mut retry = Evaluation::new(&mut host, Some(limits())).unwrap();
    assert_eq!(retry.cloned(&value, 0).unwrap(), value);
}

#[test]
fn many_empty_values_and_reference_depth_have_independent_limits() {
    let mut host = CallbackHost(no_call, || Ok(()));
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_nodes: 8,
            ..limits()
        }),
    )
    .unwrap();
    assert!(evaluation
        .cloned(&json!([null, null, null, null, null, null, null, null]), 0)
        .unwrap_err()
        .contains("node limit"));
    let mut value = Value::Null;
    for _ in 0..8 {
        value = Value::Array(vec![value]);
    }
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_depth: 4,
            ..limits()
        }),
    )
    .unwrap();
    assert!(evaluation
        .cloned(&value, 0)
        .unwrap_err()
        .contains("depth limit"));
}

#[test]
fn retained_bindings_keys_and_exports_share_the_cumulative_budget() {
    let mut host = CallbackHost(no_call, || Ok(()));
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_bytes: 1024,
            ..limits()
        }),
    )
    .unwrap();
    let mut bindings = Bindings::new();
    for name in ["a", "b", "c"] {
        evaluation.key(name).unwrap();
        let value = resolve_with(&json!("x".repeat(32)), &bindings, &mut evaluation, 0).unwrap();
        bindings.insert(name.into(), value);
    }
    let error = resolve_with(
        &json!({"export":{"$ref":"a"}}),
        &bindings,
        &mut evaluation,
        0,
    )
    .unwrap_err();
    assert!(error.contains("byte limit"));
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_bytes: 256,
            ..limits()
        }),
    )
    .unwrap();
    assert!(evaluation
        .key(&"k".repeat(129))
        .unwrap_err()
        .contains("byte limit"));
}

#[test]
fn cancellation_and_deadline_are_observed_inside_let_assert_and_export_without_calls() {
    let array = json!((0..64).collect::<Vec<_>>());
    for (steps, exports, phase) in [
        (json!([{"let":{"a":array}}]), json!({}), "let"),
        (
            json!([{"assert":array,"equals":array}]),
            json!({}),
            "assert",
        ),
        (
            json!([{"note":"no host"}]),
            json!({"values":array}),
            "export",
        ),
    ] {
        let script = script(steps, exports);
        // Count source admission checkpoints so the stop occurs in evaluation,
        // not initial source accounting. This does no expression expansion.
        let visits = Cell::new(0);
        let mut host = CallbackHost(no_call, || {
            visits.set(visits.get() + 1);
            Ok(())
        });
        Evaluation::new(&mut host, Some(generous()))
            .unwrap()
            .account(&script.document, 0)
            .unwrap();
        let stop_at = visits.get() + 20;
        for reason in ["cancelled", "deadline exceeded"] {
            visits.set(0);
            let error = run_bounded(&script, no_call, RunOptions::default(), generous(), || {
                visits.set(visits.get() + 1);
                if visits.get() == stop_at {
                    Err(reason.into())
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
            assert!(error.contains(reason), "{phase}: {error}");
            assert_eq!(visits.get(), stop_at, "{phase} must stop immediately");
            if phase != "export" {
                assert!(error.contains("stopped at step 1"), "{error}");
            }
        }
        assert!(
            run_bounded(&script, no_call, RunOptions::default(), generous(), || Ok(
                ()
            ))
            .is_ok()
        );
    }
}

#[test]
fn predicate_and_pointer_budget_failures_are_not_swallowed_as_no_match() {
    let mut host = CallbackHost(no_call, || Err("cancelled".into()));
    let mut evaluation = Evaluation::new(&mut host, Some(limits())).unwrap();
    assert_eq!(
        matches_with(
            &json!({"a":1}),
            &json!({"/a":1}),
            &Bindings::new(),
            &mut evaluation,
            0
        )
        .unwrap_err(),
        "cancelled"
    );
    let mut host = CallbackHost(no_call, || Ok(()));
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_bytes: 1,
            ..limits()
        }),
    )
    .unwrap();
    assert!(matches_with(
        &json!({"a":1}),
        &json!({"/a":1}),
        &Bindings::new(),
        &mut evaluation,
        0
    )
    .unwrap_err()
    .contains("byte limit"));
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_work: 8,
            ..limits()
        }),
    )
    .unwrap();
    let predicate = json!({"$and": vec![json!({"/a":1}); 32]});
    assert!(matches_with(
        &json!({"a":1}),
        &predicate,
        &Bindings::new(),
        &mut evaluation,
        0
    )
    .unwrap_err()
    .contains("work limit"));
}

#[test]
fn host_results_are_bounded_before_binding_copies_and_text_decoding() {
    let script = script(
        json!([{"id":"call","call":{"group":"g","operation":"make","arguments":{}},"bind":{"copy":""}}]),
        json!({"result":{"$ref":"copy"}}),
    );
    let options = EvaluationLimits {
        max_bytes: 32 * 1024,
        max_nodes: 80,
        ..limits()
    };
    let error = run_bounded(
        &script,
        |_, _, _| Ok(json!(vec![0; 96])),
        RunOptions::default(),
        options,
        || Ok(()),
    )
    .unwrap_err();
    assert!(error.contains("node limit"), "{error}");
    let error = run_bounded(
        &script,
        |_, _, _| Ok(Value::String("[0]".repeat(32))),
        RunOptions::default(),
        options,
        || Ok(()),
    )
    .unwrap_err();
    assert!(
        error.contains("node limit"),
        "text must reserve before parsing: {error}"
    );
    let report = run_bounded(
        &script,
        |_, _, _| Ok(Value::String("{\"id\":7}".into())),
        RunOptions::default(),
        generous(),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(report["exports"]["result"]["id"], 7);
}

#[test]
fn bounded_diagnostics_do_not_serialize_value_trees() {
    let script = script(
        json!([{"assert":"a".repeat(1024),"equals":"b".repeat(1024)}]),
        json!({}),
    );
    let error = run_bounded(&script, no_call, RunOptions::default(), generous(), || {
        Ok(())
    })
    .unwrap_err();
    assert!(
        error.contains("Assertion failed") && error.len() < 120,
        "{error}"
    );
}

#[test]
fn bounded_and_trusted_execution_agree_for_select_count_project_and_calls() {
    let script = script(
        json!([
            {"let":{"values":[{"id":1},{"id":2}]}},
            {"let":{"chosen":{"$select":{"from":{"$ref":"values"},"where":{"/id":2},"pointer":"/id"}}}},
            {"assert":{"$count":{"$ref":"values"}},"equals":2},
            {"id":"made","call":{"group":"g","operation":"make","arguments":{"id":{"$ref":"chosen"}}}}
        ]),
        json!({"made":{"$ref":"made"},"point":{"$project":{"point":[3,4,0],"basis":{"origin":[0,0,0],"u":[1,0,0],"v":[0,1,0]}}}}),
    );
    let plain = run_with_progress(
        &script,
        |_, args, _| Ok(args["arguments"].clone()),
        RunOptions::default(),
    )
    .unwrap();
    let bounded = run_bounded(
        &script,
        |_, args, _| Ok(args["arguments"].clone()),
        RunOptions::default(),
        generous(),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(plain["exports"], bounded["exports"]);
    assert_eq!(bounded["exports"]["point"], json!({"x":3.0,"y":4.0}));
}

#[test]
fn project_checkpoint_cancellation_failure_and_retry_use_one_interpreter() {
    #[derive(Default)]
    struct Host {
        cancelled: bool,
        loaded: usize,
        checks: usize,
    }
    impl ProjectReplayHost for Host {
        fn checkpoint(&mut self) -> Result<(), String> {
            self.checks += 1;
            if self.cancelled && self.loaded > 0 {
                Err("closed".into())
            } else {
                Ok(())
            }
        }
        fn validate_call(&mut self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn load_project_baseline(&mut self) -> Result<(), String> {
            self.loaded += 1;
            Ok(())
        }
        fn call(&mut self, _: &str, _: Value, _: RunProgress) -> Result<Value, String> {
            panic!("no CAD calls")
        }
    }
    let source = json!({"version":2,"name":"Retry","starting_state":"project","steps":[{"let":{"a":7}},{"assert":{"$ref":"a"},"equals":7}],"exports":{"value":{"$ref":"a"}}}).to_string();
    let script = ProjectScript::parse(&source).unwrap();
    let mut host = Host {
        cancelled: true,
        ..Host::default()
    };
    assert_eq!(
        script
            .run_bounded(&mut host, RunOptions::default(), limits())
            .unwrap_err(),
        "closed"
    );
    assert_eq!(host.loaded, 1);
    host.cancelled = false;
    let report = script
        .run_bounded(&mut host, RunOptions::default(), limits())
        .unwrap();
    assert_eq!(host.loaded, 2);
    assert_eq!(report["exports"]["value"], 7);
    assert_eq!(report["project_baseline_loaded"], true);
    assert!(host.checks > 10);
}

#[test]
fn arithmetic_overflow_and_invalid_depth_fail_closed_without_allocations() {
    let mut host = CallbackHost(no_call, || Ok(()));
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_bytes: usize::MAX,
            max_nodes: usize::MAX,
            ..limits()
        }),
    )
    .unwrap();
    evaluation.charge(usize::MAX, usize::MAX).unwrap();
    assert!(evaluation.charge(1, 0).unwrap_err().contains("node limit"));
    assert!(evaluation.charge(0, 1).unwrap_err().contains("byte limit"));
    for depth in [0, 129, usize::MAX] {
        assert!(Evaluation::new(
            &mut host,
            Some(EvaluationLimits {
                max_depth: depth,
                ..limits()
            })
        )
        .is_err());
    }
}

#[test]
fn expected_reference_copy_budget_is_propagated_through_every_and_select() {
    let bindings = Bindings::from([("big".into(), json!(vec![0; 32]))]);
    let mut host = CallbackHost(no_call, || Ok(()));
    for predicate in [
        json!({"/values":{"$ref":"big"}}),
        json!({"$every":{"path":"/items","where":{"/values":{"$ref":"big"}}}}),
    ] {
        let mut evaluation = Evaluation::new(
            &mut host,
            Some(EvaluationLimits {
                max_nodes: 8,
                ..limits()
            }),
        )
        .unwrap();
        let error = matches_with(
            &json!({"values":[],"items":[{"values":[]}]}),
            &predicate,
            &bindings,
            &mut evaluation,
            0,
        )
        .unwrap_err();
        assert!(error.contains("node limit"), "{error}");
    }
    let mut evaluation = Evaluation::new(
        &mut host,
        Some(EvaluationLimits {
            max_nodes: 12,
            ..limits()
        }),
    )
    .unwrap();
    let error = resolve_with(&json!({"$select":{"from":[{"values":[]}],"where":{"/values":{"$ref":"big"}},"take":"first"}}), &bindings, &mut evaluation, 0).unwrap_err();
    assert!(error.contains("node limit"), "{error}");
}

#[test]
fn every_selector_checkpoint_propagates_cancellation_and_retry_succeeds() {
    let script = script(
        json!([
            {"let":{"rows":[{"id":1,"items":[{"ok":true},{"ok":true}]},{"id":2,"items":[{"ok":true}]}]}},
            {"let":{"ids":{"$select":{"from":{"$ref":"rows"},"where":{"$every":{"path":"/items","where":{"/ok":true}}},"take":"all","pointer":"/id"}}}},
            {"assert":{"$ref":"ids"},"equals":[1,2]}
        ]),
        json!({"ids":{"$ref":"ids"}}),
    );
    let calls = Cell::new(0);
    let report = run_bounded(&script, no_call, RunOptions::default(), generous(), || {
        calls.set(calls.get() + 1);
        Ok(())
    })
    .unwrap();
    assert_eq!(report["exports"]["ids"], json!([1, 2]));
    let checkpoints = calls.get();
    assert!(checkpoints < 1000, "keep this regression small");
    for stop in 1..=checkpoints {
        calls.set(0);
        let error = run_bounded(&script, no_call, RunOptions::default(), generous(), || {
            calls.set(calls.get() + 1);
            if calls.get() == stop {
                Err("cancel checkpoint".into())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        assert!(
            error.contains("cancel checkpoint"),
            "checkpoint {stop}: {error}"
        );
        assert_eq!(calls.get(), stop);
    }
    assert!(
        run_bounded(&script, no_call, RunOptions::default(), generous(), || Ok(
            ()
        ))
        .is_ok()
    );
}

#[test]
fn bounded_no_match_and_ambiguous_diagnostics_do_not_expand_candidates() {
    for take in ["first", "one"] {
        let where_test = if take == "first" {
            json!({"/missing":0})
        } else {
            json!({})
        };
        let expression = json!({"$select":{"from":[{"name":"x".repeat(2048)},{"name":"y".repeat(2048)}],"where":where_test,"take":take}});
        let mut host = CallbackHost(no_call, || Ok(()));
        let error = resolve_with(
            &expression,
            &Bindings::new(),
            &mut Evaluation::new(&mut host, Some(generous())).unwrap(),
            0,
        )
        .unwrap_err();
        assert!(error.len() < 80, "{error}");
        assert!(!error.contains("Candidates") && !error.contains("Values present"));
    }
}
