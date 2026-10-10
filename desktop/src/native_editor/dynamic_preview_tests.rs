use super::*;

struct Fixture {
    engine: AppState,
    bridge: SessionBridgeState,
    owner: DocumentContext,
    app: App,
    camera: Entity,
    editor: Editor,
    canvas: InterfaceRect,
}

fn call(engine: &AppState, method: &str, arguments: Value) -> Value {
    crate::session_bridge::parse_engine_envelope(engine.engine_call(method, &arguments.to_string()))
        .unwrap()
}

impl Fixture {
    fn new(tool: CreateTool) -> Self {
        let engine = AppState::new();
        call(
            &engine,
            "begin_sketch",
            json!({"type":"origin_plane","plane":"xy"}),
        );
        let bridge = SessionBridgeState::default();
        let owner = bridge.native_document_context("main", &engine).unwrap();
        let mut app = native_viewport::interface_scene_fixture();
        native_viewport::apply_interface_model(app.world_mut(), engine.viewport_frame()).unwrap();
        let canvas = InterfaceRect {
            x: 0.,
            y: 0.,
            width: 2000.,
            height: 1000.,
        };
        native_viewport::apply_interface_viewport(app.world_mut(), canvas, 1.).unwrap();
        native_viewport::apply_interface_view(
            app.world_mut(),
            &owner.document_id,
            Some(ViewportCamera {
                position: [0., 0., 1000.],
                target: [0.; 3],
                up: [0., 1., 0.],
                ..default()
            }),
            None,
        )
        .unwrap();
        let camera = app.world_mut().spawn(InterfaceCamera).id();
        let mut editor = Editor {
            stamp: Some(stamp(&engine, &bridge, &owner, None).unwrap()),
            ..default()
        };
        editor.draft.select(Some(tool));
        editor.draft.prepare(SketchPoint::ZERO, false).unwrap();
        if matches!(tool, CreateTool::Slot(_)) {
            editor
                .draft
                .prepare(SketchPoint::new(20., 0.), false)
                .unwrap();
        }
        Self {
            engine,
            bridge,
            owner,
            app,
            camera,
            editor,
            canvas,
        }
    }

    fn move_to(&mut self, raw: SketchPoint, ctrl: bool) -> HashMap<SizeField, String> {
        preview(
            self.app.world_mut(),
            &self.engine,
            &self.bridge,
            &self.owner,
            &mut self.editor,
            raw,
            ctrl,
        )
        .unwrap();
        synchronize(
            self.app.world_mut(),
            self.camera,
            &self.engine,
            &self.owner,
            &self.editor,
            self.canvas,
        )
        .unwrap();
        assert_eq!(
            self.editor.draft.cursor,
            Some(raw),
            "retain the engine's raw hint"
        );
        let [_, endpoint] = self.editor.draft.resolved_preview.unwrap();
        let rendered = native_viewport::interface_preview_snapshot(self.app.world());
        let positions = &rendered.points[0].positions;
        let last = &positions[positions.len() - 3..];
        let basis = self.editor.stamp.as_ref().unwrap().basis.unwrap();
        let expected = basis.to_3d([endpoint.x, endpoint.y]);
        assert!(last
            .iter()
            .zip(expected)
            .all(|(&a, b)| (f64::from(a) - b).abs() < 1e-5));
        let world = self.app.world();
        world
            .resource::<DynamicPanel>()
            .fields
            .iter()
            .map(|(&field, &entity)| {
                let Field::Text { value, .. } =
                    &world.get::<InterfaceControl>(entity).unwrap().field
                else {
                    panic!("Drawing dimensions must be text fields");
                };
                (field, value.clone())
            })
            .collect()
    }

    fn lock(&mut self, field: SizeField, text: &str) {
        let generation = self.editor.draft.generation;
        set(
            self.app.world_mut(),
            (&self.engine, &self.bridge, &self.owner),
            &mut self.editor,
            generation,
            field,
            text.into(),
            || Ok(()),
        )
        .unwrap();
    }
}

fn expect(values: &HashMap<SizeField, String>, field: SizeField, expected: f64) {
    let actual = values[&field].parse::<f64>().unwrap();
    assert!(
        (actual - expected).abs() <= 0.00051,
        "{field:?}: {actual} vs {expected}"
    );
}

#[test]
fn live_readouts_stay_on_the_snapped_geometry_for_every_dimensioned_tool() {
    for tool in [
        CreateTool::Line,
        CreateTool::Rectangle(RectangleMode::TwoPoint),
        CreateTool::Rectangle(RectangleMode::Center),
        CreateTool::Circle(CircleMode::CenterDiameter),
        CreateTool::Circle(CircleMode::TwoPoint),
        CreateTool::Slot(SlotMode::CenterToCenter),
        CreateTool::Slot(SlotMode::Overall),
        CreateTool::Slot(SlotMode::CenterPoint),
    ] {
        let mut fixture = Fixture::new(tool);
        let before = fixture.engine.engine_call("project_export_model", "");
        let values = fixture.move_to(SketchPoint::new(30.4, 10.3), false);
        assert_eq!(
            fixture.editor.draft.resolved_preview.unwrap()[1],
            SketchPoint::new(30., 10.)
        );
        match tool {
            CreateTool::Line => {
                expect(&values, SizeField::Length, 1000_f64.sqrt());
                expect(&values, SizeField::Angle, 10_f64.atan2(30.).to_degrees());
            }
            CreateTool::Rectangle(mode) => {
                let factor = if mode == RectangleMode::Center {
                    2.
                } else {
                    1.
                };
                expect(&values, SizeField::Width, 30. * factor);
                expect(&values, SizeField::Height, 10. * factor);
            }
            CreateTool::Circle(mode) => {
                let factor = if mode == CircleMode::CenterDiameter {
                    2.
                } else {
                    1.
                };
                expect(&values, SizeField::Diameter, 1000_f64.sqrt() * factor);
            }
            CreateTool::Slot(_) => expect(&values, SizeField::Width, 20.),
            _ => unreachable!(),
        }
        assert_eq!(
            fixture.move_to(SketchPoint::new(29.6, 9.7), false),
            values,
            "{tool:?}"
        );
        assert_eq!(
            fixture.engine.engine_call("project_export_model", ""),
            before
        );
    }
}

#[test]
fn line_readouts_follow_existing_points_inference_and_snap_suppression() {
    let mut fixture = Fixture::new(CreateTool::Line);
    call(
        &fixture.engine,
        "add_point",
        json!({"position":{"x":27.,"y":13.},"ctrl_held":true}),
    );
    let values = fixture.move_to(SketchPoint::new(27.4, 13.3), false);
    assert_eq!(
        fixture.editor.draft.resolved_preview.unwrap()[1],
        SketchPoint::new(27., 13.)
    );
    expect(&values, SizeField::Length, 898_f64.sqrt());
    expect(&values, SizeField::Angle, 13_f64.atan2(27.).to_degrees());
    call(&fixture.engine, "set_grid_snap", json!({"enabled":false}));
    for (raw, endpoint, angle) in [
        (SketchPoint::new(30.4, 0.3), SketchPoint::new(30.4, 0.), 0.),
        (SketchPoint::new(0.3, 30.4), SketchPoint::new(0., 30.4), 90.),
    ] {
        let values = fixture.move_to(raw, false);
        assert_eq!(fixture.editor.draft.resolved_preview.unwrap()[1], endpoint);
        expect(&values, SizeField::Length, 30.4);
        expect(&values, SizeField::Angle, angle);
    }
    let raw = SketchPoint::new(30.4, 0.3);
    let values = fixture.move_to(raw, true);
    assert_eq!(fixture.editor.draft.resolved_preview.unwrap()[1], raw);
    expect(&values, SizeField::Length, raw.length());
    expect(&values, SizeField::Angle, raw.y.atan2(raw.x).to_degrees());
}

#[test]
fn partial_locks_measure_the_resolved_preview_and_preserve_typed_expressions() {
    let mut line = Fixture::new(CreateTool::Line);
    line.move_to(SketchPoint::new(30.4, 10.3), false);
    line.lock(SizeField::Angle, "45");
    let values = line.move_to(SketchPoint::new(30.4, 10.3), false);
    let [anchor, endpoint] = line.editor.draft.resolved_preview.unwrap();
    assert_eq!(values[&SizeField::Angle], "45");
    assert!((endpoint.x - endpoint.y).abs() < 1e-9);
    expect(&values, SizeField::Length, anchor.distance(endpoint));
    assert_ne!(
        values[&SizeField::Length],
        format!("{:.3}", SketchPoint::new(30.4, 10.3).length())
    );
    for mode in [RectangleMode::TwoPoint, RectangleMode::Center] {
        let mut rectangle = Fixture::new(CreateTool::Rectangle(mode));
        rectangle.move_to(SketchPoint::new(30.4, 10.3), false);
        rectangle.lock(SizeField::Width, "5 + 2");
        let values = rectangle.move_to(SketchPoint::new(30.4, 10.3), false);
        assert_eq!(values[&SizeField::Width], "5 + 2");
        expect(
            &values,
            SizeField::Height,
            if mode == RectangleMode::Center {
                20.
            } else {
                10.
            },
        );
    }
}

#[test]
fn changing_or_canceling_a_gesture_retires_its_resolved_measurements() {
    let mut fixture = Fixture::new(CreateTool::Line);
    fixture.move_to(SketchPoint::new(30.4, 10.3), false);
    fixture.editor.draft.escape();
    assert!(fixture.editor.draft.resolved_preview.is_none());
    fixture
        .editor
        .draft
        .select(Some(CreateTool::Rectangle(RectangleMode::TwoPoint)));
    fixture
        .editor
        .draft
        .prepare(SketchPoint::new(10., 20.), false)
        .unwrap();
    assert!(fixture.editor.draft.resolved_preview.is_none());
    let values = fixture.move_to(SketchPoint::new(40.4, 30.3), false);
    expect(&values, SizeField::Width, 30.);
    expect(&values, SizeField::Height, 10.);
}
