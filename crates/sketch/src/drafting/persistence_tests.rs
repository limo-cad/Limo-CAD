use super::*;
use crate::{drawing_commands::DrawingCommand, SketchManager};
use limo_cad_solid::{CommitKernelRequest, KernelSceneDto};

fn manager() -> SketchManager {
    let mut manager = SketchManager::new();
    manager
        .drawing_command(
            serde_json::from_value(serde_json::json!({
                "type":"create_sheet", "arguments":{
                    "name":"Floorplan", "format":"a4", "orientation":"landscape"
                }
            }))
            .unwrap(),
        )
        .unwrap();
    let mut drawing = manager.drawing_document();
    drawing.drafting = Some(Document {
        next_id: 2,
        viewports: vec![Viewport {
            id: ViewportId(1),
            sheet_id: drawing.sheets[0].id,
            model_center_mm: [0., 0.],
            paper_center_mm: [100., 100.],
            rotation_rad: 0.,
            paper_mm_per_model_mm: 0.02,
            clip_mm: [10., 10., 180., 120.],
            hidden_layers: vec![],
        }],
        ..Document::default()
    });
    manager.set_drawing_document(drawing).unwrap();
    manager
}

#[test]
fn drafting_and_sheet_links_survive_project_replacement() {
    let manager = manager();
    let before = manager.export_project_model().unwrap();
    let mut loaded = SketchManager::new();
    let plan = loaded.prepare_load_project(before.clone()).unwrap();
    assert!(plan.jobs.is_empty());
    loaded
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![],
                errors: vec![],
            },
        })
        .unwrap();
    assert_eq!(loaded.drawing_document(), manager.drawing_document());
    let after = loaded.export_project_model().unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&before).unwrap(),
        serde_json::from_str::<serde_json::Value>(&after).unwrap()
    );
}

#[test]
fn schema_15_migrates_without_drafting_and_disguised_drafting_rejects() {
    let mut old: serde_json::Value =
        serde_json::from_str(&SketchManager::new().export_project_model().unwrap()).unwrap();
    old["schema_version"] = 15.into();
    let decoded = crate::project::decode_project(&old.to_string()).unwrap();
    assert_eq!(decoded.schema_version, 16);
    assert!(decoded.drawings.drafting.is_none());
    let mut disguised: serde_json::Value =
        serde_json::from_str(&manager().export_project_model().unwrap()).unwrap();
    disguised["schema_version"] = 15.into();
    assert!(crate::project::decode_project(&disguised.to_string())
        .unwrap_err()
        .contains("Older project schema"));
    disguised["schema_version"] = 17.into();
    assert!(crate::project::decode_project(&disguised.to_string())
        .unwrap_err()
        .contains("not supported"));
}

#[test]
fn malformed_load_omission_and_linked_sheet_deletion_are_atomic() {
    let mut manager = manager();
    let before = manager.export_project_model().unwrap();
    let mut invalid: serde_json::Value = serde_json::from_str(&before).unwrap();
    invalid["drawings"]["drafting"]["viewports"][0]["sheet_id"] = 99.into();
    assert!(manager.prepare_load_project(invalid.to_string()).is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
    let mut omitted = manager.drawing_document();
    omitted.drafting = None;
    assert!(manager.set_drawing_document(omitted).is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
    assert!(manager
        .drawing_command(DrawingCommand::DeleteSheet(
            crate::drawing_commands::SheetTarget { sheet_id: 1 }
        ))
        .is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
}

#[test]
fn paired_snapshot_retains_drafting_and_unrelated_domain_intent() {
    let mut original = manager();
    original
        .replace_project_intents(vec![limo_cad_project_file::SnapshotIntent {
            name: "intent/vendor-drafting-test.cbor".into(),
            payload: vec![0xa0],
        }])
        .unwrap();
    let snapshot = original.export_project_snapshot().unwrap();
    let mut loaded = SketchManager::new();
    let plan = loaded.prepare_load_project_snapshot(&snapshot).unwrap();
    assert!(plan.jobs.is_empty());
    loaded
        .commit_solid(CommitKernelRequest {
            transaction_id: plan.transaction_id,
            scene: KernelSceneDto {
                bodies: vec![],
                errors: vec![],
            },
        })
        .unwrap();
    assert_eq!(loaded.drawing_document(), original.drawing_document());
    assert_eq!(loaded.project_intents(), original.project_intents());
}
