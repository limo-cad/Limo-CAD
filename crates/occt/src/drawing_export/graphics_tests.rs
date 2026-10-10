use super::*;
use serde_json::json;

fn hollow() -> (DrawingDocumentDto, SolidSceneDto, DrawingProjectionDto) {
    let (mut doc, scene, mut projection) = tests::fixture(20.);
    projection.bounds = [0., 0., 10., 10.];
    projection.section = serde_json::from_value(json!([
        {"points":[[0.,0.],[10.,0.],[10.,10.],[0.,10.],[0.,0.]]},
        {"points":[[3.,3.],[7.,3.],[7.,7.],[3.,7.],[3.,3.]]}
    ]))
    .unwrap();
    doc.sheets[0].style.hatch.width_mm = 0.31;
    doc.sheets[0].style.hatch.dash_mm = vec![1.25, 0.75];
    (doc, scene, projection)
}

fn section(doc: &mut DrawingDocumentDto, removed: bool) -> DrawingViewDto {
    let anchor = |endpoint| json!({"body_id":1,"edge_id":1,"edge_key":"bottom","endpoint":endpoint,"fallback_point":[999.,999.,999.]});
    let child: DrawingViewDto = serde_json::from_value(json!({
        "id":2,"name":"Section A-A","kind":"section","direction":[0.,-1.,0.],"up":[0.,0.,1.],
        "position":[200.,140.],"scale":0.5,
        "derivation":{"type":if removed {"removed_section"} else {"section"},
            "parent_view_id":1,"first":anchor("start"),"second":anchor("end"),
            "label":"Section A-A","hatch_angle_deg":17.,"hatch_spacing_mm":2.}
    }))
    .unwrap();
    doc.sheets[0].views.insert(0, child.clone());
    doc.next_view_id = 3;
    child
}

#[test]
fn horizontal_hatch_preserves_exact_export_points_styles_and_void() {
    let (doc, _, projection) = hollow();
    let sheet = &doc.sheets[0];
    let items = section_hatch(
        &sheet.views[0],
        &projection,
        &sheet.style.hatch,
        HatchPattern {
            angle_deg: 0.,
            spacing_mm: 5.,
        },
        &mut PaperGraphicsBudget::default(),
    )
    .unwrap();
    let expected = [
        [[90., 60.], [110., 60.]],
        [[90., 65.], [110., 65.]],
        [[90., 70.], [96., 70.]],
        [[104., 70.], [110., 70.]],
        [[90., 75.], [110., 75.]],
    ];
    assert_eq!(items.len(), expected.len());
    for (item, points) in items.iter().zip(expected) {
        assert_eq!(
            item,
            &PaperPrimitive::Line {
                points: points.to_vec(),
                layer: "HATCH",
                width: 0.31,
                dash: vec![1.25, 0.75]
            }
        );
    }
}

#[test]
fn non45_degree_workspace_and_export_patterns_are_explicit_and_do_not_change_intent() {
    let (mut doc, _, projection) = hollow();
    doc.sheets[0].style.hatch_spacing_mm = 5.;
    let child = section(&mut doc, false);
    let before = serde_json::to_value(&doc).unwrap();
    let sheet = &doc.sheets[0];
    let patterns = [
        HatchPattern {
            angle_deg: 17.,
            spacing_mm: 2.,
        },
        HatchPattern {
            angle_deg: 107.,
            spacing_mm: 5.,
        },
    ];
    let mut counts = Vec::new();
    for pattern in patterns {
        let items = section_hatch(
            &child,
            &projection,
            &sheet.style.hatch,
            pattern,
            &mut PaperGraphicsBudget::default(),
        )
        .unwrap();
        assert!(!items.is_empty());
        let angle = pattern.angle_deg.to_radians();
        let u = [angle.cos(), angle.sin()];
        let n = [-angle.sin(), angle.cos()];
        for item in &items {
            let PaperPrimitive::Line { points, .. } = item else {
                panic!("Hatch must be lines")
            };
            let delta = [points[1][0] - points[0][0], points[1][1] - points[0][1]];
            assert!((delta[0] * u[1] - delta[1] * u[0]).abs() < 1e-9);
            let lattice = (points[0][0] * n[0] + points[0][1] * n[1]) / pattern.spacing_mm;
            assert!((lattice - lattice.round()).abs() < 1e-9);
            let mid = [
                (points[0][0] + points[1][0]) * 0.5,
                (points[0][1] + points[1][1]) * 0.5,
            ];
            assert!(
                !(mid[0] > 199. && mid[0] < 201. && mid[1] > 139. && mid[1] < 141.),
                "Hatched the hollow section"
            );
        }
        counts.push(items.len());
    }
    assert_ne!(counts[0], counts[1]);
    assert_eq!(serde_json::to_value(&doc).unwrap(), before);
}

#[test]
fn exporter_keeps_derivation_spacing_and_horizontal_angle_convention() {
    let (mut doc, scene, projection) = hollow();
    doc.sheets[0].style.hatch_spacing_mm = 5.;
    let child = section(&mut doc, false);
    let sheet = &doc.sheets[0];
    let expected = section_hatch(
        &child,
        &projection,
        &sheet.style.hatch,
        HatchPattern {
            angle_deg: 17.,
            spacing_mm: 2.,
        },
        &mut PaperGraphicsBudget::default(),
    )
    .unwrap();
    let expected = svg(
        &Paper {
            size: [297., 210.],
            items: expected,
        },
        &sheet.style.font_family,
    );
    let exported = export_sheet(
        &doc,
        &scene,
        &AssemblyDocumentDto::default(),
        &DrawingExportRequest {
            sheet_id: 1,
            format: DrawingExportFormat::Svg,
        },
        |_| Ok(projection.clone()),
    )
    .unwrap();
    let hatch_lines = |text: &str| {
        text.lines()
            .filter(|line| line.contains("data-layer=\"HATCH\""))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(hatch_lines(&exported), hatch_lines(&expected));
    assert!(!hatch_lines(&exported).is_empty());
}

#[test]
fn section_generation_rejects_work_scratch_and_output_before_exceeding_budget() {
    let (doc, _, projection) = hollow();
    let sheet = &doc.sheets[0];
    let pattern = HatchPattern {
        angle_deg: 17.,
        spacing_mm: 0.5,
    };
    for (limits, message) in [
        (
            PaperGraphicsLimits {
                work: 10,
                ..Default::default()
            },
            "work limit",
        ),
        (
            PaperGraphicsLimits {
                scratch_bytes: 1,
                ..Default::default()
            },
            "scratch memory",
        ),
        (
            PaperGraphicsLimits {
                primitives: 2,
                ..Default::default()
            },
            "primitive limit",
        ),
        (
            PaperGraphicsLimits {
                points: 3,
                ..Default::default()
            },
            "point limit",
        ),
        (
            PaperGraphicsLimits {
                retained_bytes: 1,
                ..Default::default()
            },
            "retained memory",
        ),
    ] {
        let mut budget = PaperGraphicsBudget::new(limits);
        let error = section_hatch(
            &sheet.views[0],
            &projection,
            &sheet.style.hatch,
            pattern,
            &mut budget,
        )
        .unwrap_err();
        assert!(error.contains(message), "{error}");
        let usage = budget.usage();
        assert!(usage.primitives <= limits.primitives && usage.points <= limits.points);
        assert!(usage.retained_bytes <= limits.retained_bytes && usage.work <= limits.work);
        assert!(usage.peak_scratch_bytes <= limits.scratch_bytes);
        if message == "work limit" || message == "scratch memory" {
            assert_eq!(usage.primitives, 0);
        }
    }
}

#[test]
fn section_generation_rejects_open_or_extreme_boundaries_without_panicking() {
    let (doc, _, mut projection) = hollow();
    let sheet = &doc.sheets[0];
    projection.section[0].points.pop();
    assert!(section_hatch(
        &sheet.views[0],
        &projection,
        &sheet.style.hatch,
        HatchPattern {
            angle_deg: 0.,
            spacing_mm: 1.
        },
        &mut PaperGraphicsBudget::default()
    )
    .unwrap_err()
    .contains("open"));
    for pattern in [
        HatchPattern {
            angle_deg: 0.,
            spacing_mm: 0.,
        },
        HatchPattern {
            angle_deg: f64::INFINITY,
            spacing_mm: 1.,
        },
        HatchPattern {
            angle_deg: 0.,
            spacing_mm: 1e-300,
        },
    ] {
        assert!(section_hatch(
            &sheet.views[0],
            &projection,
            &sheet.style.hatch,
            pattern,
            &mut PaperGraphicsBudget::default()
        )
        .is_err());
    }
    let mut view = sheet.views[0].clone();
    view.position = [1e300, -1e300];
    assert!(section_hatch(
        &view,
        &projection,
        &sheet.style.hatch,
        HatchPattern {
            angle_deg: 17.,
            spacing_mm: 1.
        },
        &mut PaperGraphicsBudget::default()
    )
    .is_err());
}

#[test]
fn section_source_borrows_parent_projection_preserves_styles_and_rejects_stale_refs() {
    for removed in [false, true] {
        let (mut doc, scene, projection) = tests::fixture(20.);
        let child = section(&mut doc, removed);
        let before = serde_json::to_value(&doc).unwrap();
        let sheet = &doc.sheets[0];
        let lookups = std::cell::Cell::new(0);
        let mut budget = PaperGraphicsBudget::default();
        let items = derived_source_graphics(
            &child,
            sheet,
            |id| {
                assert_eq!(id, 1);
                lookups.set(lookups.get() + 1);
                Some(&projection)
            },
            &scene,
            &AssemblyDocumentDto::default(),
            &mut budget,
        )
        .unwrap();
        assert_eq!(lookups.get(), 1);
        assert_eq!(items.len(), 5);
        assert_eq!(
            items[0],
            PaperPrimitive::Line {
                points: vec![[76., 70.], [124., 70.]],
                layer: "CUTTING_PLANE",
                width: sheet.style.cutting_plane.width_mm,
                dash: sheet.style.cutting_plane.dash_mm.clone()
            }
        );
        assert_eq!(
            items
                .iter()
                .filter(|p| matches!(p, PaperPrimitive::Triangle { .. }))
                .count(),
            2
        );
        assert!(
            matches!(&items[3],PaperPrimitive::Text{point,value,centered:true,height,..}
            if *point==[72.,70.] && value=="A-A" && *height==sheet.style.text_height_mm)
        );
        assert_eq!(budget.usage().primitives, 5);
        assert_eq!(serde_json::to_value(&doc).unwrap(), before);
        let mut excluded = projection.clone();
        excluded.anchors.clear();
        assert!(derived_source_graphics(
            &child,
            sheet,
            |_| Some(&excluded),
            &scene,
            &AssemblyDocumentDto::default(),
            &mut PaperGraphicsBudget::default()
        )
        .unwrap_err()
        .contains("parent projection"));
    }
}

#[test]
fn shared_sheet_budget_does_not_reset_for_each_source_marker() {
    let (mut doc, scene, projection) = tests::fixture(20.);
    let child = section(&mut doc, false);
    let mut budget = PaperGraphicsBudget::new(PaperGraphicsLimits {
        primitives: 7,
        ..Default::default()
    });
    let first = derived_source_graphics(
        &child,
        &doc.sheets[0],
        |_| Some(&projection),
        &scene,
        &AssemblyDocumentDto::default(),
        &mut budget,
    )
    .unwrap();
    assert_eq!(first.len(), 5);
    let error = derived_source_graphics(
        &child,
        &doc.sheets[0],
        |_| Some(&projection),
        &scene,
        &AssemblyDocumentDto::default(),
        &mut budget,
    )
    .unwrap_err();
    assert!(error.contains("primitive limit"));
    assert_eq!(budget.usage().primitives, 7);
    assert_eq!(
        first.len(),
        5,
        "A failed batch cannot alter previously returned graphics"
    );
}

#[test]
fn aggregate_append_charges_capacity_and_preserves_payloads_without_recounting() {
    fn batch(budget: &mut PaperGraphicsBudget) -> Vec<PaperPrimitive> {
        let mut graphics = graphics::Graphics::new(budget);
        graphics.label([2., 3.], "Section 零件", 4., true).unwrap();
        graphics.finish()
    }
    let mut budget = PaperGraphicsBudget::default();
    let mut destination = Vec::new();
    let first = batch(&mut budget);
    let before = budget.usage();
    budget.append(&mut destination, first).unwrap();
    let after = budget.usage();
    assert!(after.retained_bytes >= before.retained_bytes + std::mem::size_of::<PaperPrimitive>());
    assert_eq!(after.primitives, before.primitives);
    assert_eq!(after.points, before.points);
    assert_eq!(after.work, before.work + 1);
    let second = batch(&mut budget);
    let before_second = budget.usage();
    let additional = (destination.len() + second.len() - destination.capacity())
        * std::mem::size_of::<PaperPrimitive>();
    assert!(additional > 0);
    budget.append(&mut destination, second).unwrap();
    assert_eq!(destination.len(), 2);
    assert_eq!(destination[0], destination[1]);
    assert!(
        matches!(&destination[1], PaperPrimitive::Text { value, point, .. }
        if value == "Section 零件" && *point == [2., 3.])
    );
    assert_eq!(budget.usage().primitives, 2);

    let mut limited = PaperGraphicsBudget::new(PaperGraphicsLimits {
        retained_bytes: before_second.retained_bytes + additional - 1,
        ..Default::default()
    });
    let first = batch(&mut limited);
    let mut unchanged = Vec::new();
    limited.append(&mut unchanged, first).unwrap();
    let second = batch(&mut limited);
    let old_capacity = unchanged.capacity();
    assert!(limited
        .append(&mut unchanged, second)
        .unwrap_err()
        .contains("retained memory"));
    assert_eq!(unchanged.len(), 1);
    assert_eq!(unchanged.capacity(), old_capacity);
    assert_eq!(unchanged[0], destination[0]);
    assert_eq!(limited.usage().retained_bytes, before_second.retained_bytes);
}

fn tiled_fixture(angle_deg: f64) -> (DrawingViewDto, DrawingProjectionDto) {
    let (doc, _, mut projection) = hollow();
    let mut view = doc.sheets[0].views[0].clone();
    let angle = angle_deg.to_radians();
    let (u, n) = ([angle.cos(), angle.sin()], [-angle.sin(), angle.cos()]);
    let polygon = |first: f64, last: f64, half: f64| {
        [
            [first, -half],
            [last, -half],
            [last, half],
            [first, half],
            [first, -half],
        ]
        .map(|[along, across]| {
            [
                along * u[0] + across * n[0],
                -(along * u[1] + across * n[1]),
            ]
        })
    };
    projection.section = serde_json::from_value(json!([
        {"points":polygon(-6.7,13.3,5.)},
        {"points":polygon(1.3,4.7,2.)}
    ]))
    .unwrap();
    let points = projection.section.iter().flat_map(|line| &line.points);
    projection.bounds = points.fold(
        [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ],
        |mut b, p| {
            b[0] = b[0].min(p[0]);
            b[1] = b[1].min(p[1]);
            b[2] = b[2].max(p[0]);
            b[3] = b[3].max(p[1]);
            b
        },
    );
    let b = projection.bounds;
    view.position = [(b[0] + b[2]) * 0.5, -(b[1] + b[3]) * 0.5];
    view.scale = 1.;
    (view, projection)
}

#[test]
fn native_tiled_hatch_rotated_17_degrees_keeps_global_phase_across_boundaries_and_voids() {
    let pattern = HatchPattern {
        angle_deg: 107.,
        spacing_mm: 4.5,
    };
    let (view, projection) = tiled_fixture(pattern.angle_deg);
    let style = DrawingLineStyleDto {
        width_mm: 0.31,
        dash_mm: vec![1.25, 0.75],
    };
    let before = serde_json::to_value((&view, &projection)).unwrap();
    let items = section_hatch_tiled(
        &view,
        &projection,
        &style,
        pattern,
        &mut PaperGraphicsBudget::default(),
    )
    .unwrap();
    let angle = pattern.angle_deg.to_radians();
    let (u, n) = ([angle.cos(), angle.sin()], [-angle.sin(), angle.cos()]);
    let mut center_line = Vec::new();
    for item in items {
        let PaperPrimitive::Line {
            points,
            dash,
            width,
            layer,
        } = item
        else {
            panic!("hatch must be lines")
        };
        assert!(
            dash.is_empty(),
            "native on-spans must not restart a renderer dash"
        );
        assert_eq!(width, style.width_mm);
        assert_eq!(layer, "HATCH");
        let across = points[0][0] * n[0] + points[0][1] * n[1];
        if across.abs() < 1e-8 {
            center_line.push(
                points
                    .iter()
                    .map(|p| p[0] * u[0] + p[1] * u[1])
                    .collect::<Vec<_>>(),
            );
        }
    }
    let expected = [
        [-6.7, -5.75],
        [-5., -4.5],
        [-4.5, -3.25],
        [-2.5, -1.25],
        [-0.5, 0.],
        [0., 1.25],
        [4.7, 5.75],
        [6.5, 7.75],
        [8.5, 9.],
        [9., 10.25],
        [11., 12.25],
        [13., 13.3],
    ];
    assert_eq!(center_line.len(), expected.len(), "{center_line:?}");
    for (actual, expected) in center_line.iter().zip(expected) {
        for (a, b) in actual.iter().zip(expected) {
            assert!((a - b).abs() < 1e-8, "{actual:?} != {expected:?}");
        }
    }
    assert_eq!(serde_json::to_value((&view, &projection)).unwrap(), before);
}

#[test]
fn native_tiled_hatch_repeats_odd_svg_dash_lists_and_keeps_solid_policy_exact() {
    let pattern = HatchPattern {
        angle_deg: 17.,
        spacing_mm: 4.5,
    };
    let (view, projection) = tiled_fixture(pattern.angle_deg);
    let style = |dash_mm| DrawingLineStyleDto {
        width_mm: 0.25,
        dash_mm,
    };
    let generate = |style: &DrawingLineStyleDto| {
        section_hatch_tiled(
            &view,
            &projection,
            style,
            pattern,
            &mut PaperGraphicsBudget::default(),
        )
        .unwrap()
    };
    assert_eq!(
        generate(&style(vec![1., 0.5, 0.75])),
        generate(&style(vec![1., 0.5, 0.75, 1., 0.5, 0.75]))
    );
    let solid = style(vec![]);
    assert_eq!(
        generate(&solid),
        section_hatch(
            &view,
            &projection,
            &solid,
            pattern,
            &mut PaperGraphicsBudget::default()
        )
        .unwrap()
    );
}

#[test]
fn native_tiled_hatch_rejects_excessive_dash_work_before_allocating_on_spans() {
    let pattern = HatchPattern {
        angle_deg: 17.,
        spacing_mm: 4.5,
    };
    let (view, projection) = tiled_fixture(pattern.angle_deg);
    let style = DrawingLineStyleDto {
        width_mm: 0.25,
        dash_mm: vec![1e-9, 1e-9],
    };
    let mut budget = PaperGraphicsBudget::new(PaperGraphicsLimits {
        work: 5_000,
        ..Default::default()
    });
    let error = section_hatch_tiled(&view, &projection, &style, pattern, &mut budget).unwrap_err();
    assert!(error.contains("work limit"), "{error}");
    assert_eq!(budget.usage().primitives, 0);
    assert_eq!(budget.usage().retained_bytes, 0);
    assert!(budget.usage().work <= 5_000);

    let mut limited = PaperGraphicsBudget::new(PaperGraphicsLimits {
        primitives: 2,
        ..Default::default()
    });
    let style = DrawingLineStyleDto {
        width_mm: 0.25,
        dash_mm: vec![1., 0.5],
    };
    assert!(
        section_hatch_tiled(&view, &projection, &style, pattern, &mut limited)
            .unwrap_err()
            .contains("primitive limit")
    );
    assert_eq!(limited.usage().primitives, 2);
}
