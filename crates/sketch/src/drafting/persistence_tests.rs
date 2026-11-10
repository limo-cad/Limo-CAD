use super::*;
use crate::{drawing_commands::DrawingCommand, SketchManager};
use limo_cad_solid::{CommitKernelRequest, KernelSceneDto};

fn sheet_manager() -> SketchManager {
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
    manager
}

fn manager() -> SketchManager {
    let mut manager = sheet_manager();
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
fn blocked_sheet_and_hidden_layer_deletes_name_dependencies_and_can_be_recovered() {
    use super::commands::Command;
    let mut manager = manager();
    manager
        .drafting_command(Command::CreateLayer {
            name: "Retained geometry".into(),
            visible: true,
            locked: false,
            plottable: true,
            style: Style {
                line_type: None,
                color_rgb: [0, 0, 0],
                width_mm: 0.25,
            },
        })
        .unwrap();
    manager
        .drafting_command(Command::CreateEntity {
            space: Space::Paper(1),
            layer: LayerId(2),
            style: None,
            content: Content::Curve {
                curve: Curve::Line {
                    start: [0., 0.],
                    end: [10., 0.],
                },
            },
        })
        .unwrap();
    let mut viewport = manager.drawing_document().drafting.unwrap().viewports[0].clone();
    viewport.hidden_layers.push(LayerId(2));
    manager
        .drafting_command(Command::UpdateViewport(viewport))
        .unwrap();
    let before = manager.export_project_model().unwrap();
    let delete = DrawingCommand::DeleteSheet(crate::drawing_commands::SheetTarget { sheet_id: 1 });
    let error = format!("{:?}", manager.drawing_command(delete.clone()).unwrap_err());
    for expected in [
        "Floorplan",
        "drafting entities [3]",
        "viewports [1]",
        "Remove those objects",
    ] {
        assert!(error.contains(expected), "{error}");
    }
    let output_error = manager
        .drawing_document()
        .ensure_projected_sheet_supported(1)
        .unwrap_err();
    for expected in [
        "Floorplan",
        "drafting entities [3]",
        "viewports [1]",
        "client that supports authored drafting",
    ] {
        assert!(output_error.contains(expected), "{output_error}");
    }
    assert_eq!(manager.export_project_model().unwrap(), before);
    manager
        .drafting_command(Command::DeleteEntity { id: EntityId(3) })
        .unwrap();
    let before = manager.export_project_model().unwrap();
    let error = format!(
        "{:?}",
        manager
            .drafting_command(Command::DeleteLayer { id: LayerId(2) })
            .unwrap_err()
    );
    for expected in [
        "Retained geometry",
        "viewport hidden-layer lists [1]",
        "remove the layer",
    ] {
        assert!(error.contains(expected), "{error}");
    }
    assert_eq!(manager.export_project_model().unwrap(), before);
    manager
        .drafting_command(Command::DeleteViewport { id: ViewportId(1) })
        .unwrap();
    manager
        .drafting_command(Command::DeleteLayer { id: LayerId(2) })
        .unwrap();
    manager.drawing_command(delete).unwrap();
    assert!(manager.drawing_document().sheets.is_empty());
}

#[test]
fn rich_drafting_graph_survives_project_replacement() {
    let mut manager = sheet_manager();
    let mut graph = super::tests::fixture();
    graph.layers.push(Layer {
        id: LayerId(101),
        name: "Locked".into(),
        locked: true,
        ..graph.layers[0].clone()
    });
    graph.references.push(Reference {
        id: ReferenceId(100),
        locator: "linked-plan.limo".into(),
        mode: ReferenceMode::Overlay,
        source_mm_per_unit: 1.,
        resolution: Resolution::Unresolved {
            diagnostic: "Not loaded".into(),
        },
    });
    graph.entities.push(Entity {
        id: EntityId(102),
        space: Space::Model,
        layer: LayerId(1),
        style: None,
        content: Content::Curve {
            curve: Curve::Nurbs {
                degree: 2,
                control_points: vec![[0., 0.], [1., 2.], [2., 0.]],
                weights: vec![1.; 3],
                knots: vec![0., 0., 0., 1., 1., 1.],
            },
        },
    });
    graph.entities.push(Entity {
        id: EntityId(103),
        space: Space::Model,
        layer: LayerId(1),
        style: None,
        content: Content::Reference {
            reference: ReferenceId(100),
            transform: Affine::default(),
        },
    });
    graph.viewports.push(Viewport {
        id: ViewportId(6),
        sheet_id: 1,
        model_center_mm: [0., 0.],
        paper_center_mm: [100., 100.],
        rotation_rad: 0.,
        paper_mm_per_model_mm: 0.1,
        clip_mm: [10., 10., 180., 120.],
        hidden_layers: vec![LayerId(101)],
    });
    graph.next_id = 104;
    let mut drawing = manager.drawing_document();
    drawing.drafting = Some(graph);
    manager.set_drawing_document(drawing).unwrap();
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
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&loaded.export_project_model().unwrap()).unwrap(),
        serde_json::from_str::<serde_json::Value>(&before).unwrap()
    );
}

#[test]
fn viewport_clip_uses_the_owning_sheets_physical_bounds_atomically() {
    use super::commands::Command;
    let mut manager = manager();
    let mut viewport = manager.drawing_document().drafting.unwrap().viewports[0].clone();
    viewport.clip_mm = [0., 0., 297., 210.];
    manager
        .drafting_command(Command::UpdateViewport(viewport.clone()))
        .unwrap();
    for clip in [
        [-1., 0., 100., 100.],
        [296., 0., 2., 100.],
        [0., 209., 100., 2.],
    ] {
        let before = manager.export_project_model().unwrap();
        viewport.clip_mm = clip;
        let error = format!(
            "{:?}",
            manager
                .drafting_command(Command::UpdateViewport(viewport.clone()))
                .unwrap_err()
        );
        assert!(
            error.contains("viewport 1") && error.contains("Floorplan"),
            "{error}"
        );
        assert_eq!(manager.export_project_model().unwrap(), before);
    }
    let mut drawing = manager.drawing_document();
    drawing.sheets[0].orientation = crate::DrawingSheetOrientation::Portrait;
    drawing.drafting.as_mut().unwrap().viewports[0].clip_mm = [0., 0., 210., 297.];
    manager.set_drawing_document(drawing).unwrap();
    let mut drawing = manager.drawing_document();
    drawing.sheets[0].format = crate::DrawingSheetFormat::Letter;
    drawing.drafting.as_mut().unwrap().viewports[0].clip_mm = [0.1, 0., 215.8, 279.4];
    manager.set_drawing_document(drawing).unwrap();
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
