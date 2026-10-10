use super::*;

#[test]
fn plan_has_all_cases_and_reverses_repeat_order() {
    let plan = cases();
    assert_eq!(plan.len(), 16);
    assert_eq!(
        plan.iter().filter(|case| case.source == "baseline").count(),
        8
    );
    for pair in plan.as_chunks::<2>().0 {
        assert_eq!(pair[0].scenario, pair[1].scenario);
        assert_eq!(pair[0].instances, pair[1].instances);
        assert_eq!(
            pair[0].source,
            if pair[0].repeat == 1 {
                "baseline"
            } else {
                "candidate"
            }
        );
        assert_ne!(pair[0].source, pair[1].source);
    }
}

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    let evidence = root.path();
    fs::create_dir(evidence.join("builds")).unwrap();
    fs::create_dir(evidence.join("inputs")).unwrap();
    fs::create_dir(evidence.join("binaries")).unwrap();
    for source in ["baseline", "candidate"] {
        fs::write(
            evidence.join("builds").join(format!("{source}.sha")),
            "a".repeat(40),
        )
        .unwrap();
        fs::write(
            evidence.join("binaries").join(format!("{source}-native")),
            source,
        )
        .unwrap();
    }
    for name in ["part-a", "part-b", "sheets"] {
        fs::write(evidence.join("inputs").join(format!("{name}.nbcad")), name).unwrap();
    }
    for case in cases() {
        let out = evidence.join(case.relative());
        fs::create_dir_all(&out).unwrap();
        let hashes: Vec<_> = case
            .inputs()
            .iter()
            .map(|name| {
                crate::hash::file(&evidence.join("inputs").join(format!("{name}.nbcad"))).unwrap()
            })
            .collect();
        fs::write(out.join("report.json"), r#"{"completed":true}"#).unwrap();
        let metadata = json!({"scenario":case.scenario,"input_sha256":hashes,
            "declared_commit":"a".repeat(40),"declared_build_profile":"release","shell":"native",
            "instances":case.instances,"cycles":20,
            "binary_sha256":crate::hash::file(&evidence.join("binaries").join(format!("{}-native",case.source))).unwrap()});
        fs::write(
            out.join("metadata.json"),
            serde_json::to_vec(&metadata).unwrap(),
        )
        .unwrap();
        for instance in 0..case.instances {
            let directory = out.join(format!("instance-{instance}"));
            fs::create_dir(&directory).unwrap();
            for (model, name) in case.inputs().iter().enumerate() {
                fs::write(
                    directory.join(format!("loaded-{model}.json")),
                    serde_json::to_vec(&json!({"model":name})).unwrap(),
                )
                .unwrap();
            }
        }
    }
    root
}

#[test]
fn aggregation_requires_complete_matching_inputs_models_and_build_receipts() {
    let root = fixture();
    let evidence = root.path();
    let binaries = evidence.join("binaries");
    assert_eq!(
        aggregate(evidence, &binaries, 0).unwrap()["complete_matched_measurement"],
        true
    );
    assert_eq!(
        aggregate(evidence, &binaries, 1).unwrap()["complete_matched_measurement"],
        false
    );
    let out = evidence.join(cases()[0].relative());
    let mut metadata = json_file(&out.join("metadata.json")).unwrap();
    metadata["declared_commit"] = json!("b".repeat(40));
    fs::write(
        out.join("metadata.json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    assert_eq!(
        aggregate(evidence, &binaries, 0).unwrap()["complete_matched_measurement"],
        false
    );
    metadata["declared_commit"] = json!("a".repeat(40));
    fs::write(
        out.join("metadata.json"),
        serde_json::to_vec(&metadata).unwrap(),
    )
    .unwrap();
    fs::write(out.join("instance-0/loaded-0.json"), "{}").unwrap();
    assert_eq!(
        aggregate(evidence, &binaries, 0).unwrap()["complete_matched_measurement"],
        false
    );
}

#[test]
fn malformed_or_missing_receipts_are_retained_as_incomplete_evidence() {
    let root = fixture();
    let evidence = root.path();
    let out = evidence.join(cases()[0].relative());
    fs::write(out.join("report.json"), "malformed").unwrap();
    fs::remove_file(out.join("metadata.json")).unwrap();
    let summary = aggregate(evidence, &evidence.join("binaries"), 0).unwrap();
    assert_eq!(summary["complete_matched_measurement"], false);
    assert_eq!(
        summary["scenarios"]["document-tabs"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}
