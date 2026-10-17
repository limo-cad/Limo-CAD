//! This fixture was saved by the pre-region-selection engine: selecting its
//! inner loop produced the surrounding ring, with BodyId(1).
use limo_cad_sketch::{SketchManager, PROJECT_SCHEMA_VERSION};
use limo_cad_solid::{CommitKernelRequest, KernelJobDto, KernelSceneDto, SetRollbackRequest};
use serde_json::{json, Value};

const LEGACY: &str = include_str!("fixtures/legacy_nested_profile_v14.json");

#[test]
fn legacy_aliases_deduplicate_in_order_without_reassigning_reserved_bodies() {
    for selected in [json!([1]), json!([1, 0]), json!([0, 1])] {
        let mut model: Value = serde_json::from_str(LEGACY).unwrap();
        model["extrudes"][0]["profile_indices"] = selected;
        model["extrudes"][0]["new_body_ids"] = json!([1, 42]);
        // All history is initially rolled back; migration must include it.
        model["document"]["history"]["rollback_index"] = json!(0);
        let mut manager = SketchManager::new();
        let plan = manager.prepare_load_project(model.to_string()).unwrap();
        assert!(plan.jobs.is_empty() && plan.errors.is_empty());
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene: KernelSceneDto {
                    bodies: vec![],
                    errors: vec![],
                },
            })
            .unwrap();
        let saved: Value = serde_json::from_str(&manager.export_project_model().unwrap()).unwrap();
        assert_eq!(saved["schema_version"], PROJECT_SCHEMA_VERSION);
        assert_eq!(saved["extrudes"][0]["profile_indices"], json!([0]));
        assert_eq!(saved["extrudes"][0]["new_body_ids"], json!([1, 42]));
        let plan = manager
            .prepare_set_rollback(SetRollbackRequest { rollback_index: 2 })
            .unwrap();
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("expected Extrude")
        };
        assert_eq!(job.profiles.len(), 1);
        assert_eq!(job.profiles[0].profile_index, 0);
        assert_eq!(job.profiles[0].holes[0].profile_index, 1);
        assert_eq!(job.result_body_ids[0].0, 1);
    }
}

#[test]
fn direct_regions_remain_inner_regions_in_current_and_initial_pr_files() {
    for version in [14, PROJECT_SCHEMA_VERSION] {
        let mut model: Value = serde_json::from_str(LEGACY).unwrap();
        model["schema_version"] = json!(version);
        model["extrudes"][0]["merge_profiles"] = json!(false);
        let mut manager = SketchManager::new();
        let plan = manager.prepare_load_project(model.to_string()).unwrap();
        assert!(plan.errors.is_empty(), "{:?}", plan.errors);
        let KernelJobDto::Extrude(job) = &plan.jobs[0] else {
            panic!("expected Extrude")
        };
        assert_eq!(job.profiles[0].profile_index, 1);
        assert!(job.profiles[0].holes.is_empty());
        assert_eq!(job.result_body_ids[0].0, 1);
    }
}
