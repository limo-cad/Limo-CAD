use super::*;

fn layer() -> Command {
    Command::CreateLayer {
        name: "Geometry".into(),
        visible: true,
        locked: false,
        plottable: true,
        style: Style {
            line_type: None,
            color_rgb: [0, 0, 0],
            width_mm: 0.25,
        },
    }
}
fn line(layer: LayerId, space: Space) -> Command {
    Command::CreateEntity {
        space,
        layer,
        style: None,
        content: Content::Curve {
            curve: Curve::Line {
                start: [0., 0.],
                end: [10., 0.],
            },
        },
    }
}
fn graph(manager: &SketchManager) -> Document {
    manager.drawing_document().drafting.unwrap()
}

#[test]
fn failed_commands_do_not_create_graphs_consume_ids_or_delete_dependencies() {
    let mut manager = SketchManager::new();
    assert!(manager
        .drafting_command(line(LayerId(99), Space::Model))
        .is_err());
    assert!(manager.drawing_document().drafting.is_none());
    manager.drafting_command(layer()).unwrap();
    let before = manager.export_project_model().unwrap();
    assert!(manager
        .drafting_command(line(LayerId(99), Space::Model))
        .is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
    manager
        .drafting_command(line(LayerId(1), Space::Model))
        .unwrap();
    assert_eq!(graph(&manager).entities[0].id, EntityId(2));
    let before = manager.export_project_model().unwrap();
    assert!(manager
        .drafting_command(Command::DeleteLayer { id: LayerId(1) })
        .is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
    manager
        .drafting_command(Command::DeleteEntity { id: EntityId(2) })
        .unwrap();
    manager
        .drafting_command(line(LayerId(1), Space::Model))
        .unwrap();
    assert_eq!(graph(&manager).entities[0].id, EntityId(3));
}

#[test]
fn block_instances_share_definitions_and_keep_separate_attribute_values() {
    let mut manager = SketchManager::new();
    manager.drafting_command(layer()).unwrap();
    manager
        .drafting_command(Command::CreateBlock {
            name: "Door".into(),
            base_point: [0., 0.],
            attributes: vec![NewAttribute {
                tag: "NUMBER".into(),
                default_value: "D0".into(),
            }],
        })
        .unwrap();
    manager
        .drafting_command(line(LayerId(1), Space::Block(BlockId(2))))
        .unwrap();
    for value in ["D1", "D2"] {
        manager
            .drafting_command(Command::CreateEntity {
                space: Space::Model,
                layer: LayerId(1),
                style: None,
                content: Content::Block {
                    block: BlockId(2),
                    transform: Affine::default(),
                    attributes: vec![AttributeValue {
                        attribute: AttributeId(3),
                        value: value.into(),
                    }],
                },
            })
            .unwrap();
    }
    manager
        .drafting_command(Command::UpdateBlock {
            id: BlockId(2),
            name: "Wide door".into(),
            base_point: [5., 0.],
        })
        .unwrap();
    let saved = graph(&manager);
    assert_eq!(saved.blocks.len(), 1);
    assert_eq!(saved.blocks[0].base_point, [5., 0.]);
    let values: Vec<_> = saved
        .entities
        .iter()
        .filter_map(|e| match &e.content {
            Content::Block {
                block, attributes, ..
            } => {
                assert_eq!(*block, BlockId(2));
                Some(attributes[0].value.as_str())
            }
            _ => None,
        })
        .collect();
    assert_eq!(values, vec!["D1", "D2"]);
    let before = manager.export_project_model().unwrap();
    assert!(manager
        .drafting_command(Command::DeleteAttribute {
            block: BlockId(2),
            id: AttributeId(3)
        })
        .is_err());
    assert!(manager
        .drafting_command(Command::DeleteBlock { id: BlockId(2) })
        .is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
}

#[test]
fn typed_edits_obey_shared_layer_locks() {
    let mut manager = SketchManager::new();
    manager.drafting_command(layer()).unwrap();
    manager
        .drafting_command(line(LayerId(1), Space::Model))
        .unwrap();
    let mut locked = graph(&manager).layers[0].clone();
    locked.locked = true;
    manager
        .drafting_command(Command::UpdateLayer(locked.clone()))
        .unwrap();
    let before = manager.export_project_model().unwrap();
    let mut edited = graph(&manager).entities[0].clone();
    edited.content = Content::Curve {
        curve: Curve::Line {
            start: [0., 0.],
            end: [25., 0.],
        },
    };
    assert!(manager
        .drafting_command(Command::UpdateEntity(edited.clone()))
        .is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
    locked.locked = false;
    manager
        .drafting_command(Command::UpdateLayer(locked))
        .unwrap();
    manager
        .drafting_command(Command::UpdateEntity(edited.clone()))
        .unwrap();
    assert_eq!(graph(&manager).entities[0], edited);
}

#[test]
fn viewport_and_reference_commands_preserve_explicit_dependency_state() {
    let mut manager = SketchManager::new();
    manager.drafting_command(layer()).unwrap();
    let viewport = Command::CreateViewport {
        sheet_id: 1,
        model_center_mm: [0., 0.],
        paper_center_mm: [100., 100.],
        rotation_rad: 0.,
        paper_mm_per_model_mm: 0.02,
        clip_mm: [10., 10., 180., 120.],
        hidden_layers: vec![],
    };
    assert!(manager.drafting_command(viewport.clone()).is_err());
    manager
        .drawing_command(
            serde_json::from_value(serde_json::json!({"type":"create_sheet",
        "arguments":{"name":"Layout","format":"a4","orientation":"landscape"}}))
            .unwrap(),
        )
        .unwrap();
    manager.drafting_command(viewport).unwrap();
    assert_eq!(graph(&manager).viewports[0].id, ViewportId(2));
    manager
        .drafting_command(Command::AttachReference {
            locator: "floorplan.limo".into(),
            mode: ReferenceMode::Attach,
            source_mm_per_unit: 1.,
        })
        .unwrap();
    assert!(matches!(
        graph(&manager).references[0].resolution,
        Resolution::Unresolved { .. }
    ));
    manager
        .drafting_command(Command::CreateEntity {
            space: Space::Model,
            layer: LayerId(1),
            style: None,
            content: Content::Reference {
                reference: ReferenceId(3),
                transform: Affine::default(),
            },
        })
        .unwrap();
    assert!(manager
        .drafting_command(Command::DetachReference { id: ReferenceId(3) })
        .is_err());
    manager
        .drafting_command(Command::SetReferenceMode {
            id: ReferenceId(3),
            mode: ReferenceMode::Overlay,
        })
        .unwrap();
    assert!(serde_json::from_value::<Command>(serde_json::json!({"type":"attach_reference","arguments":{
        "locator":"x.limo","mode":"attach","source_mm_per_unit":1.,"resolution":{"kind":"resolved"}
    }})).is_err());
}

#[test]
fn allocator_exhaustion_is_an_unchanged_failure() {
    let mut manager = SketchManager::new();
    let mut drawing = manager.drawing_document();
    drawing.drafting = Some(Document {
        next_id: u64::MAX,
        ..Document::default()
    });
    manager.set_drawing_document(drawing).unwrap();
    let before = manager.export_project_model().unwrap();
    assert!(manager.drafting_command(layer()).is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
}

fn issued_sheets(manager: &mut SketchManager) {
    let mut drawing = manager.drawing_document();
    for sheet in &mut drawing.sheets {
        sheet.release.status = crate::drawing::DrawingReleaseStatus::Released;
        sheet.release.released_revision = "A".into();
        sheet.release.released_at = "2026-10-18".into();
    }
    manager.set_drawing_document(drawing).unwrap();
}
fn two_sheets() -> SketchManager {
    let mut manager = SketchManager::new();
    for name in ["Authored", "Technical"] {
        manager.drawing_command(serde_json::from_value(serde_json::json!({
            "type":"create_sheet","arguments":{"name":name,"format":"a4","orientation":"landscape"}
        })).unwrap()).unwrap();
    }
    manager.drafting_command(layer()).unwrap();
    manager
}
#[test]
fn paper_creation_and_last_item_removal_revoke_release_only_on_authored_sheets() {
    use crate::drawing::DrawingReleaseStatus::{Draft, Released};
    let mut manager = two_sheets();
    issued_sheets(&mut manager);
    manager
        .drafting_command(line(LayerId(1), Space::Paper(1)))
        .unwrap();
    let drawing = manager.drawing_document();
    assert_eq!(drawing.sheets[0].release.status, Draft);
    assert_eq!(drawing.sheets[1].release.status, Released);
    assert_eq!(drawing.sheets[0].release.released_revision, "A");
    issued_sheets(&mut manager);
    manager
        .drafting_command(Command::DeleteEntity { id: EntityId(2) })
        .unwrap();
    let drawing = manager.drawing_document();
    assert_eq!(drawing.sheets[0].release.status, Draft);
    assert_eq!(drawing.sheets[1].release.status, Released);
}
#[test]
fn model_edits_invalidate_viewport_release_but_noops_and_failures_do_not() {
    use crate::drawing::DrawingReleaseStatus::{Draft, Released};
    let mut manager = two_sheets();
    manager
        .drafting_command(Command::CreateViewport {
            sheet_id: 1,
            model_center_mm: [0., 0.],
            paper_center_mm: [100., 100.],
            rotation_rad: 0.,
            paper_mm_per_model_mm: 0.02,
            clip_mm: [10., 10., 180., 120.],
            hidden_layers: vec![],
        })
        .unwrap();
    issued_sheets(&mut manager);
    let unchanged = manager.drawing_document();
    manager.set_drawing_document(unchanged.clone()).unwrap();
    assert_eq!(manager.drawing_document(), unchanged);
    let before = manager.export_project_model().unwrap();
    assert!(manager
        .drafting_command(line(LayerId(999), Space::Model))
        .is_err());
    assert_eq!(manager.export_project_model().unwrap(), before);
    manager
        .drafting_command(line(LayerId(1), Space::Model))
        .unwrap();
    let drawing = manager.drawing_document();
    assert_eq!(drawing.sheets[0].release.status, Draft);
    assert_eq!(drawing.sheets[1].release.status, Released);
    issued_sheets(&mut manager);
    // Raw native document edits use the same invalidation gate as typed commands.
    let mut edited = manager.drawing_document();
    edited.drafting.as_mut().unwrap().layers[0].style.width_mm = 0.5;
    manager.set_drawing_document(edited).unwrap();
    let drawing = manager.drawing_document();
    assert_eq!(drawing.sheets[0].release.status, Draft);
    assert_eq!(drawing.sheets[1].release.status, Released);
}
