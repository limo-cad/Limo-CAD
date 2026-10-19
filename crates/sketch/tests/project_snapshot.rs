use limo_cad_core::{OriginPlane, PlaneRef};
use limo_cad_fem::{
    SavedThermalStudy, SolveOptions, ThermalMesh, ThermalStudyDocument, ThermalStudyInput,
    THERMAL_STUDY_ENTRY,
};
use limo_cad_project_file::{ProjectSnapshot, SnapshotIntent};
use limo_cad_sketch::SketchManager;
use limo_cad_solid::{CommitKernelRequest, KernelSceneDto};
fn thermal(name: &str) -> SnapshotIntent {
    let doc = ThermalStudyDocument {
        studies: vec![SavedThermalStudy {
            id: 1,
            name: name.into(),
            input: ThermalStudyInput {
                mesh: ThermalMesh {
                    points_m: vec![],
                    elements: vec![],
                },
                materials: vec![],
                prescribed_k: vec![],
                nodal_load_w: vec![],
                surfaces: vec![],
                options: SolveOptions::default(),
            },
        }],
    };
    SnapshotIntent {
        name: THERMAL_STUDY_ENTRY.into(),
        payload: doc.encode_cbor().unwrap(),
    }
}
fn manager(name: &str) -> SketchManager {
    let mut manager = SketchManager::new();
    manager.set_document_name(name.into()).unwrap();
    manager
        .replace_project_intents(vec![
            thermal(name),
            SnapshotIntent {
                name: "intent/vendor".into(),
                payload: vec![0xff, 0x00],
            },
        ])
        .unwrap();
    manager
}
fn commit(manager: &mut SketchManager, id: u64) -> Result<(), limo_cad_sketch::SessionError> {
    manager
        .commit_solid(CommitKernelRequest {
            transaction_id: id,
            scene: KernelSceneDto {
                bodies: vec![],
                errors: vec![],
            },
        })
        .map(|_| ())
}
#[test]
fn complete_snapshot_keeps_known_and_unknown_intent_and_blocks_lossy_export() {
    let mut m = manager("A");
    assert!(!m.is_blank_for_script());
    assert!(m
        .export_project_model()
        .unwrap_err()
        .to_string()
        .contains("complete project snapshot"));
    let s = m.export_project_snapshot().unwrap();
    assert_eq!(s.intents(), m.project_intents());
    assert_eq!(
        ProjectSnapshot::decode_cbor(&s.encode_cbor().unwrap()).unwrap(),
        s
    );
    assert!(!m
        .replace_project_intents(m.project_intents().to_vec())
        .unwrap());
    assert!(m.replace_project_intents(vec![]).unwrap());
    assert!(m.export_project_model().is_ok());
}
#[test]
fn staged_load_adopts_model_and_domains_only_at_matching_commit() {
    let source = manager("Incoming");
    let snapshot = source.export_project_snapshot().unwrap();
    let mut target = manager("Original");
    let before = target.export_project_snapshot().unwrap();
    let plan = target.prepare_load_project_snapshot(&snapshot).unwrap();
    assert!(plan.jobs.is_empty());
    assert_eq!(target.export_project_snapshot().unwrap(), before);
    assert!(target.replace_project_intents(vec![]).is_err());
    assert!(commit(&mut target, plan.transaction_id + 1).is_err());
    assert_eq!(target.export_project_snapshot().unwrap(), before);
    commit(&mut target, plan.transaction_id).unwrap();
    assert_eq!(target.document_dto().name, "Incoming");
    assert_eq!(target.project_intents(), snapshot.intents());
}
#[test]
fn cancelling_staged_snapshot_preserves_both_live_halves() {
    let incoming = manager("Incoming").export_project_snapshot().unwrap();
    let mut target = manager("Original");
    let before = target.export_project_snapshot().unwrap();
    let plan = target.prepare_load_project_snapshot(&incoming).unwrap();
    target.cancel_solid_recompute(plan.transaction_id);
    assert_eq!(target.export_project_snapshot().unwrap(), before);
    assert!(commit(&mut target, plan.transaction_id).is_err());
    let retry = target.prepare_load_project_snapshot(&incoming).unwrap();
    commit(&mut target, retry.transaction_id).unwrap();
    assert_eq!(target.document_dto().name, "Incoming");
}
#[test]
fn invalid_known_domain_or_model_rejects_before_project_staging() {
    let mut target = manager("Original");
    let before = target.export_project_snapshot().unwrap();
    let bad = ProjectSnapshot::new(
        before.model_json().into(),
        vec![SnapshotIntent {
            name: THERMAL_STUDY_ENTRY.into(),
            payload: vec![0xff],
        }],
    )
    .unwrap();
    assert!(target.prepare_load_project_snapshot(&bad).is_err());
    assert_eq!(target.export_project_snapshot().unwrap(), before);
    assert!(target
        .replace_project_intents(bad.intents().to_vec())
        .is_err());
    assert_eq!(target.export_project_snapshot().unwrap(), before);
    let future_model = r#"{"format":"limo-cad-project","schema_version":999}"#;
    let bad = ProjectSnapshot::new(future_model.into(), before.intents().to_vec()).unwrap();
    assert!(target.prepare_load_project_snapshot(&bad).is_err());
    assert_eq!(target.export_project_snapshot().unwrap(), before);
    let valid = manager("Valid").export_project_snapshot().unwrap();
    let plan = target.prepare_load_project_snapshot(&valid).unwrap();
    commit(&mut target, plan.transaction_id).unwrap();
}
#[test]
fn active_sketch_rejects_domain_edits_and_explicit_new_clears_old_intent_on_commit() {
    let mut target = manager("Original");
    let intents = target.project_intents().to_vec();
    target
        .begin_sketch(PlaneRef::OriginPlane {
            plane: OriginPlane::Xy,
        })
        .unwrap();
    assert!(target.replace_project_intents(vec![]).is_err());
    assert_eq!(target.project_intents(), intents);
    target.end_sketch().unwrap();
    let plan = target.prepare_new_project().unwrap();
    assert_eq!(target.project_intents(), intents);
    commit(&mut target, plan.transaction_id).unwrap();
    assert!(target.project_intents().is_empty());
    assert!(target.export_project_model().is_ok());
}

#[test]
fn pending_geometry_recompute_blocks_intent_edits_and_complete_snapshot_capture() {
    let mut target = manager("Original");
    let before = target.export_project_snapshot().unwrap();
    let plan = target.prepare_recompute().unwrap();
    assert!(target.replace_project_intents(vec![]).is_err());
    assert!(target.export_project_snapshot().is_err());
    target.cancel_solid_recompute(plan.transaction_id);
    assert_eq!(target.export_project_snapshot().unwrap(), before);
}
