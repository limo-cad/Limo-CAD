use limo_cad_core::{MaterialDetails, MaterialProperty, MaterialSource, MaterialValue};
use limo_cad_fem::*;
fn input() -> ThermalStudyInput {
    ThermalStudyInput {
        mesh: ThermalMesh {
            points_m: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            elements: vec![ThermalTet {
                nodes: [0, 1, 2, 3],
                material: 0,
                source_w_m3: 0.,
            }],
        },
        materials: vec![ThermalMaterial {
            details: MaterialDetails {
                kind: "other".into(),
                catalog_id: "test.conductor".into(),
                properties: vec![MaterialProperty {
                    name: "ThermalConductivity".into(),
                    value: MaterialValue::Number(2.),
                    unit: "W/(m*K)".into(),
                    context: "test".into(),
                    source_id: "fixture".into(),
                }],
                sources: vec![MaterialSource {
                    id: "fixture".into(),
                    repository: "test".into(),
                    revision: "1".repeat(40),
                    path: "conductor".into(),
                    sha256: "2".repeat(64),
                    license: "test".into(),
                    author: "test".into(),
                    reference: String::new(),
                }],
                ..Default::default()
            },
            context: Some("test".into()),
            source_id: Some("fixture".into()),
        }],
        prescribed_k: vec![(0, 300.), (1, 310.), (2, 300.), (3, 300.)],
        nodal_load_w: vec![0.; 4],
        surfaces: vec![],
        options: SolveOptions::default(),
    }
}
fn solved() -> ThermalStudy {
    let mut study = ThermalStudy::new(input());
    let completion = study.begin_solve().run();
    assert!(study.finish(completion));
    assert_eq!(study.status(), &ThermalStudyStatus::Solved);
    assert!(study.result_is_current());
    assert_eq!(
        study.result().unwrap().result().element_flux_w_m2,
        vec![[-20., 0., 0.]]
    );
    study
}
#[test]
fn worker_snapshot_retains_catalog_provenance_and_result() {
    let mut study = ThermalStudy::new(input());
    let job = study.begin_solve();
    let completion = std::thread::spawn(move || job.run()).join().unwrap();
    assert!(study.finish(completion));
    let result = study.result().unwrap();
    assert_eq!(result.input(), study.input());
    assert_eq!(
        result.input().materials[0].details.sources[0].sha256,
        "2".repeat(64)
    );
    assert_eq!(result.result().temperature_k, vec![300., 310., 300., 300.]);
}
#[test]
fn edits_cancel_old_work_and_undo_restores_exact_input_freshness() {
    let mut study = solved();
    let original = study.input().clone();
    let old = study.begin_solve();
    let mut edited = original.clone();
    edited.prescribed_k[1].1 = 320.;
    assert!(study.replace_input(edited));
    assert!(old.is_cancelled());
    assert!(!study.result_is_current());
    assert!(!study.finish(old.run()));
    assert_eq!(study.status(), &ThermalStudyStatus::Idle);
    assert_eq!(study.result().unwrap().result().temperature_k[1], 310.);
    study.replace_input(original);
    assert!(study.result_is_current());
}
#[test]
fn completed_old_attempt_cannot_replace_newer_result() {
    let mut study = solved();
    let old = study.begin_solve().run();
    let mut edited = study.input().clone();
    edited.prescribed_k[1].1 = 330.;
    study.replace_input(edited);
    let latest = study.begin_solve().run();
    assert!(study.finish(latest));
    assert!(!study.finish(old));
    assert_eq!(study.result().unwrap().result().temperature_k[1], 330.);
    assert_eq!(study.status(), &ThermalStudyStatus::Solved);
}
#[test]
fn same_input_new_attempt_and_foreign_owner_reject_old_completions() {
    let mut a = solved();
    let mut b = ThermalStudy::new(input());
    let foreign = a.begin_solve().run();
    let b_job = b.begin_solve();
    assert!(!b.finish(foreign));
    assert_eq!(b.status(), &ThermalStudyStatus::Running);
    assert!(b.finish(b_job.run()));
    let old = a.begin_solve().run();
    let latest = a.begin_solve();
    assert!(!a.finish(old));
    assert_eq!(a.status(), &ThermalStudyStatus::Running);
    assert!(a.finish(latest.run()));
}
#[test]
fn cancellation_and_owner_drop_retire_jobs_without_losing_good_result() {
    let mut study = solved();
    let completion = study.begin_solve().run();
    assert!(study.cancel());
    assert!(!study.cancel());
    assert!(!study.finish(completion));
    assert_eq!(study.status(), &ThermalStudyStatus::Cancelled);
    assert!(study.result_is_current());
    let job = study.begin_solve();
    drop(study);
    assert!(job.is_cancelled());
    // A cancelled job can still finish; its retired owner cannot adopt it.
    let _completion = job.run();
}
#[test]
fn invalid_draft_solve_preserves_last_success_and_reports_failure() {
    let mut study = solved();
    let mut bad = study.input().clone();
    bad.materials[0].context = Some("missing".into());
    study.replace_input(bad);
    let completion = study.begin_solve().run();
    assert!(study.finish(completion));
    assert!(matches!(study.status(), ThermalStudyStatus::Failed(_)));
    assert!(!study.result_is_current());
    assert_eq!(study.result().unwrap().result().temperature_k[1], 310.);
    assert_eq!(
        study.result().unwrap().input().materials[0]
            .context
            .as_deref(),
        Some("test")
    );
}
#[test]
fn no_op_edit_preserves_running_job() {
    let mut study = solved();
    let job = study.begin_solve();
    assert!(!study.replace_input(study.input().clone()));
    assert!(!job.is_cancelled());
    assert_eq!(study.status(), &ThermalStudyStatus::Running);
    assert!(study.finish(job.run()));
}
#[test]
fn every_input_category_invalidates_retained_result() {
    let mut study = solved();
    let original = study.input().clone();
    type InputEdit = Box<dyn Fn(&mut ThermalStudyInput)>;
    let changes: Vec<InputEdit> = vec![
        Box::new(|i| i.mesh.points_m[1][0] = 2.),
        Box::new(|i| i.mesh.elements[0].nodes.swap(0, 1)),
        Box::new(|i| i.mesh.elements[0].material = 1),
        Box::new(|i| i.mesh.elements[0].source_w_m3 = 1.),
        Box::new(|i| i.materials[0].details.properties[0].value = MaterialValue::Number(3.)),
        Box::new(|i| i.materials[0].details.sources[0].revision = "3".repeat(40)),
        Box::new(|i| i.materials[0].context = None),
        Box::new(|i| i.materials[0].source_id = None),
        Box::new(|i| i.prescribed_k[0].1 = 301.),
        Box::new(|i| i.nodal_load_w[0] = 1.),
        Box::new(|i| {
            i.surfaces.push(ThermalSurface {
                nodes: [0, 1, 2],
                heat_flux_in_w_m2: 1.,
                convection: None,
            })
        }),
        Box::new(|i| i.options.relative_tolerance = 1e-8),
    ];
    for change in changes {
        let mut edited = original.clone();
        change(&mut edited);
        assert!(study.replace_input(edited));
        assert!(!study.result_is_current());
        study.replace_input(original.clone());
        assert!(study.result_is_current());
    }
}
