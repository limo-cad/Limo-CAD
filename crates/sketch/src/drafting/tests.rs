use super::*;

fn fixture() -> Document {
    Document {
        next_id: 100,
        layers: vec![Layer {
            id: LayerId(1),
            name: "Doors".into(),
            visible: true,
            locked: false,
            plottable: true,
            style: Style {
                line_type: None,
                color_rgb: [0, 0, 0],
                width_mm: 0.25,
            },
        }],
        blocks: vec![Block {
            id: BlockId(2),
            name: "Door".into(),
            base_point: [0., 0.],
            attributes: vec![Attribute {
                id: AttributeId(3),
                tag: "NUMBER".into(),
                default_value: "D0".into(),
            }],
        }],
        entities: vec![
            Entity {
                id: EntityId(4),
                space: Space::Block(BlockId(2)),
                layer: LayerId(1),
                style: None,
                content: Content::Curve {
                    curve: Curve::Line {
                        start: [0., 0.],
                        end: [900., 0.],
                    },
                },
            },
            Entity {
                id: EntityId(5),
                space: Space::Model,
                layer: LayerId(1),
                style: None,
                content: Content::Block {
                    block: BlockId(2),
                    transform: Affine::default(),
                    attributes: vec![AttributeValue {
                        attribute: AttributeId(3),
                        value: "D1".into(),
                    }],
                },
            },
        ],
        ..Document::default()
    }
}

#[test]
fn projected_paper_admission_is_scoped_to_authored_sheet_content() {
    let mut drawing = crate::DrawingDocumentDto::default();
    drawing.drafting = Some(fixture());
    // Uninstanced definitions and model entities have no paper representation.
    drawing.ensure_projected_sheet_supported(10).unwrap();
    let mut paper_entity = drawing.drafting.as_ref().unwrap().entities[1].clone();
    paper_entity.id = EntityId(6);
    paper_entity.space = Space::Paper(10);
    drawing
        .drafting
        .as_mut()
        .unwrap()
        .entities
        .push(paper_entity);
    assert!(drawing.ensure_projected_sheet_supported(10).is_err());
    drawing.ensure_projected_sheet_supported(11).unwrap();
    drawing.drafting.as_mut().unwrap().entities.pop();
    drawing.drafting.as_mut().unwrap().viewports.push(Viewport {
        id: ViewportId(7),
        sheet_id: 10,
        model_center_mm: [0., 0.],
        paper_center_mm: [100., 100.],
        rotation_rad: 0.,
        paper_mm_per_model_mm: 0.02,
        clip_mm: [10., 10., 180., 120.],
        hidden_layers: vec![],
    });
    assert!(drawing.ensure_projected_sheet_supported(10).is_err());
    drawing.ensure_projected_sheet_supported(11).unwrap();
}

#[test]
fn repeated_instances_preserve_definition_and_independent_attributes() {
    let mut document = fixture();
    let mut second = document.entities[1].clone();
    second.id = EntityId(6);
    if let Content::Block {
        transform,
        attributes,
        ..
    } = &mut second.content
    {
        *transform = Affine([-2., 0., 0., 3., 10000., 0.]);
        attributes[0].value = "D2".into();
    }
    document.entities.push(second);
    document.validate(&[]).unwrap();
    let encoded = serde_json::to_value(&document).unwrap();
    let decoded: Document = serde_json::from_value(encoded).unwrap();
    assert_eq!(document, decoded);
    assert_eq!(decoded.blocks.len(), 1);
    assert_ne!(decoded.entities[1].content, decoded.entities[2].content);
}

#[test]
fn identity_layer_and_attribute_rejections_leave_input_unchanged() {
    let mut document = fixture();
    document.entities[1].id = EntityId(1);
    assert!(document.validate(&[]).is_err());
    document = fixture();
    document.entities[1].layer = LayerId(99);
    assert!(document.validate(&[]).is_err());
    document = fixture();
    if let Content::Block { attributes, .. } = &mut document.entities[1].content {
        attributes.push(attributes[0].clone());
    }
    let before = document.clone();
    assert!(document.validate(&[]).is_err());
    assert_eq!(document, before);
    document = fixture();
    if let Content::Block { attributes, .. } = &mut document.entities[1].content {
        attributes[0].attribute = AttributeId(99);
    }
    assert!(document.validate(&[]).is_err());
    document = fixture();
    document.next_id = 5;
    assert!(document.validate(&[]).is_err());
}

#[test]
fn cycles_through_blocks_and_resolved_references_are_rejected() {
    let mut document = fixture();
    document.entities[1].space = Space::Block(BlockId(2));
    assert!(document.validate(&[]).unwrap_err().contains("Cyclic"));
    document = fixture();
    document.references.push(Reference {
        id: ReferenceId(6),
        locator: "floorplan.limo".into(),
        mode: ReferenceMode::Attach,
        source_mm_per_unit: 1.,
        resolution: Resolution::Resolved {
            block: BlockId(2),
            sha256: "a".repeat(64),
        },
    });
    document.entities.push(Entity {
        id: EntityId(7),
        space: Space::Block(BlockId(2)),
        layer: LayerId(1),
        style: None,
        content: Content::Reference {
            reference: ReferenceId(6),
            transform: Affine::default(),
        },
    });
    assert!(document.validate(&[]).unwrap_err().contains("Cyclic"));
    document.entities.last_mut().unwrap().space = Space::Model;
    document.validate(&[]).unwrap();
    document.references[0].resolution = Resolution::Stale {
        block: BlockId(2),
        sha256: "a".repeat(64),
        diagnostic: "Source unavailable".into(),
    };
    document.validate(&[]).unwrap();
    document.references[0].resolution = Resolution::Resolved {
        block: BlockId(2),
        sha256: "not-a-digest".into(),
    };
    assert!(document.validate(&[]).is_err());
}

#[test]
fn viewport_links_scales_and_hidden_layers_are_checked() {
    let mut document = fixture();
    document.viewports.push(Viewport {
        id: ViewportId(8),
        sheet_id: 10,
        model_center_mm: [0., 0.],
        paper_center_mm: [100., 100.],
        rotation_rad: 0.,
        paper_mm_per_model_mm: 0.02,
        clip_mm: [10., 10., 180., 120.],
        hidden_layers: vec![LayerId(1)],
    });
    assert!(document.validate(&[]).is_err());
    document.validate(&[10]).unwrap();
    document.viewports[0].paper_mm_per_model_mm = f64::NAN;
    assert!(document.validate(&[10]).is_err());
    document.viewports[0].paper_mm_per_model_mm = 0.01;
    document.viewports[0].hidden_layers.push(LayerId(1));
    assert!(document.validate(&[10]).is_err());
    document.viewports[0].hidden_layers = vec![LayerId(99)];
    assert!(document.validate(&[10]).is_err());
}

#[test]
fn exact_curve_admission_rejects_invalid_knots_and_singular_transforms() {
    let mut document = fixture();
    document.entities[0].content = Content::Curve {
        curve: Curve::Nurbs {
            degree: 2,
            control_points: vec![[0., 0.], [1., 1.], [2., 0.]],
            weights: vec![1., 0.5, 1.],
            knots: vec![0., 0., 0., 1., 1., 1.],
        },
    };
    document.validate(&[]).unwrap();
    if let Content::Curve {
        curve: Curve::Nurbs { knots, .. },
    } = &mut document.entities[0].content
    {
        knots[3] = -1.;
    }
    assert!(document.validate(&[]).is_err());
    document = fixture();
    if let Content::Block { transform, .. } = &mut document.entities[1].content {
        *transform = Affine([1., 2., 2., 4., 0., 0.]);
    }
    assert!(document.validate(&[]).is_err());
    document = fixture();
    document.entities[0].content = Content::Curve {
        curve: Curve::Polyline {
            points: vec![],
            closed: false,
        },
    };
    assert!(document.validate(&[]).is_err());
}

#[test]
fn contract_version_unknown_fields_and_text_limits_are_explicit() {
    Document::default().validate(&[]).unwrap();
    let mut document = fixture();
    document.version = 2;
    assert!(document.validate(&[]).is_err());
    document = fixture();
    let mut value = serde_json::to_value(&document).unwrap();
    value["unexpected"] = true.into();
    assert!(serde_json::from_value::<Document>(value).is_err());
    document.layers[0].name = "x".repeat(65_537);
    assert!(document.validate(&[]).is_err());
}

#[test]
fn deep_shared_definition_graph_is_iterative_and_duplicate_edges_are_safe() {
    let mut document = fixture();
    document.blocks.clear();
    document.entities.clear();
    document.next_id = 10_000;
    for i in 0..1024 {
        document.blocks.push(Block {
            id: BlockId(2 + i),
            name: format!("B{i}"),
            base_point: [0., 0.],
            attributes: vec![],
        });
        if i < 1023 {
            document.entities.push(Entity {
                id: EntityId(2000 + i),
                space: Space::Block(BlockId(2 + i)),
                layer: LayerId(1),
                style: None,
                content: Content::Block {
                    block: BlockId(3 + i),
                    transform: Affine::default(),
                    attributes: vec![],
                },
            });
        }
    }
    let mut repeated = document.entities[0].clone();
    repeated.id = EntityId(4000);
    document.entities.push(repeated);
    document.validate(&[]).unwrap();
    document.entities.push(Entity {
        id: EntityId(4001),
        space: Space::Block(BlockId(1025)),
        layer: LayerId(1),
        style: None,
        content: Content::Block {
            block: BlockId(2),
            transform: Affine::default(),
            attributes: vec![],
        },
    });
    assert!(document.validate(&[]).unwrap_err().contains("Cyclic"));
}
