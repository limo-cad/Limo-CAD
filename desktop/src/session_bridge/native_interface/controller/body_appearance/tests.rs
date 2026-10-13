use super::*;
use crate::{
    native_viewport::{
        interface_shell::{fields, InterfaceOccluder, PointerButton, PointerPhase},
        winit_host::Modifiers,
    },
    session_bridge::{native_interface::tests::Fixture, parse_engine_envelope},
};
use bevy::{
    ecs::schedule::Schedule,
    input::{mouse::MouseButtonInput, ButtonState},
    text::{EditableText, TextEdit},
    ui::{ComputedStackIndex, ComputedUiRenderTargetInfo, UiGlobalTransform, UiScale},
};
use limo_cad_core::Rgba8;
use limo_cad_interface::ControlKey;

struct Panel {
    app: App,
    services: NativeServices,
    handle: NativeInterfaceHandle,
    camera: Entity,
    bodies: Vec<u64>,
}

fn model(fixture: &Fixture) -> Value {
    parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap()
}

fn appearances(fixture: &Fixture) -> Value {
    parse_engine_envelope(fixture.engine.engine_call("body_appearances", "")).unwrap()
}

fn saved_appearance(body: u64) -> BodyAppearance {
    BodyAppearance {
        body_id: BodyId(body),
        color: Rgba8::new(24, 53, 91, 153),
        material_name: "Preserved custom blend".into(),
        filament_type: "PETG".into(),
        brand: "Bambu Lab".into(),
        color_name: "Stored blue".into(),
        filament_id: Some("preserved-vendor-id".into()),
        preset_id: None,
        density_g_cm3: Some(1.234_567_89),
        material: None,
        diameter_mm: 2.850_123_45,
    }
}

impl Panel {
    fn new(fixture: &Fixture) -> Self {
        for index in 1..=2 {
            for (operation, arguments) in [
                (
                    "sketch_begin",
                    json!({"plane":{"type":"origin_plane","plane":"xy"}}),
                ),
                (
                    "sketch_add_rectangle",
                    json!({"mode":"two_point","p1":{"x":index as f64 * 20.,"y":0.},"p2":{"x":index as f64 * 20. + 12.,"y":8.},"ctrl_held":true}),
                ),
                ("sketch_finish", json!({})),
                (
                    "solid_extrude",
                    json!({"sketch_name":format!("Sketch{index}"),"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":6.}}),
                ),
            ] {
                fixture
                    .bridge
                    .apply_native_mutation(
                        &fixture.engine,
                        &fixture.owner(),
                        operation,
                        &arguments,
                        || Ok(()),
                    )
                    .unwrap();
            }
        }
        let bodies: Vec<u64> = fixture
            .engine
            .viewport_snapshot()
            .2
            .bodies
            .iter()
            .map(|b| b.id.0)
            .collect();
        assert_eq!(bodies.len(), 2);
        for body in &bodies {
            fixture
                .bridge
                .apply_native_mutation(
                    &fixture.engine,
                    &fixture.owner(),
                    "set_body_appearance",
                    &serde_json::to_value(saved_appearance(*body)).unwrap(),
                    || Ok(()),
                )
                .unwrap();
        }
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let handle = NativeInterfaceHandle::new(|| {});
        let mut app = native_viewport::interface_scene_fixture();
        app.add_schedule(Schedule::new(Startup))
            .add_schedule(Schedule::new(Update))
            .add_schedule(Schedule::new(Last))
            .add_plugins(bevy::text::TextPlugin)
            .init_resource::<Assets<Image>>()
            .init_resource::<Time<bevy::time::Real>>()
            .init_resource::<UiScale>()
            .init_resource::<bevy::input_focus::InputFocus>()
            .insert_resource(handle.clone())
            .insert_resource(State {
                preference: preferences::Observer::at(
                    crate::session_bridge::session_root().join("body-slicer-target.json"),
                ),
                ..default()
            });
        fields::install(&mut app);
        let camera = app.world_mut().spawn(InterfaceCamera).id();
        let rendered = refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap();
        app.insert_resource(NativeRenderedDocument {
            owner: receipt.owner,
            revision: receipt.revision,
            bodies: rendered,
        });
        worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
        let mut panel = Self {
            app,
            services,
            handle,
            camera,
            bodies,
        };
        panel.select(fixture, panel.bodies[0]);
        panel.paint(fixture);
        panel
    }

    fn select(&mut self, fixture: &Fixture, body: u64) {
        let (_, _, mut view, _) = native_viewport::interface_view_snapshot(self.app.world());
        view.selected_body_ids = vec![body];
        native_viewport::apply_interface_view(
            self.app.world_mut(),
            &fixture.owner().document_id,
            None,
            Some(view),
        )
        .unwrap();
    }

    fn paint(&mut self, fixture: &Fixture) {
        synchronize(
            self.app.world_mut(),
            self.camera,
            &self.services,
            &fixture.owner(),
            1360.,
            860.,
            true,
        )
        .unwrap();
        let bounds = InterfaceRect {
            x: 0.,
            y: 0.,
            width: 1360.,
            height: 860.,
        };
        self.handle
            .present(InterfaceFrame {
                context: fixture.owner(),
                client: bounds,
                surface: bounds,
                canvases: vec![],
                surfaces: vec![Surface {
                    name: "body/appearance".into(),
                    text: None,
                }],
                modal_stack: vec![],
                document_visible: true,
            })
            .unwrap();
        let px_value = |value: Val| match value {
            Val::Px(value) => value,
            other => panic!("Expected absolute panel layout, got {other:?}"),
        };
        let layout: Vec<_> = self.app.world_mut().query_filtered::<
            (Entity, &Node, &ZIndex), Or<(With<InterfaceControl>, With<InterfaceOccluder>)>
        >().iter(self.app.world()).map(|(entity, node, z)| {
            let size = Vec2::new(px_value(node.width), px_value(node.height));
            (entity, size, Vec2::new(px_value(node.left), px_value(node.top)) + size / 2., z.0)
        }).collect();
        for (entity, size, center, z) in layout {
            self.app.world_mut().entity_mut(entity).insert((
                ComputedNode {
                    size,
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(center),
                ComputedStackIndex(z as u32),
                ComputedUiRenderTargetInfo::default(),
                InheritedVisibility::VISIBLE,
            ));
        }
        self.app.update();
        self.app
            .world_mut()
            .run_system_cached(bevy::ui::widget::update_editable_text_styles)
            .unwrap();
        self.app
            .world_mut()
            .run_system_cached(bevy::ui::widget::update_editable_text_layout)
            .unwrap();
        interface_shell::tests::publish_layout_once(self.app.world_mut(), self.handle.clone());
    }

    fn entity(&self, command: Command) -> Entity {
        self.app
            .world()
            .resource::<State>()
            .widgets
            .entity(&match command {
                Command::Field(field) => format!("appearance-field-{:?}", Some(field)),
                Command::Apply => "appearance-Apply appearance".into(),
                Command::Reset => "appearance-Reset appearance".into(),
                Command::SlicerTarget => "appearance-field-None".into(),
                Command::Details => "material-properties".into(),
                Command::Scroll(delta) => format!(
                    "appearance-{} appearance fields",
                    if delta < 0 { "Previous" } else { "More" }
                ),
                _ => panic!("This helper expects an appearance field or footer"),
            })
            .unwrap()
    }

    fn action(&self, command: Command, input: ControlInput) -> NativeInterfaceAction {
        self.handle
            .resolve_input(
                ControlKey(self.entity(command).to_bits()),
                input,
                &self.handle.frame().unwrap().context,
            )
            .unwrap()
    }

    fn reduce(&mut self, action: &NativeInterfaceAction) -> Result<Value, String> {
        super::super::reduce_control_input(
            &self.services.engine,
            &self.services.bridge,
            self.app.world_mut(),
            &self.handle,
            action,
        )
    }

    fn set(&mut self, field: Field, value: &str) -> Value {
        let action = self.action(Command::Field(field), ControlInput::SetValue(value.into()));
        self.reduce(&action).unwrap()
    }

    fn edit_buffer(&mut self, field: Field, value: &str) {
        let entity = self.entity(Command::Field(field));
        assert!(self
            .app
            .world()
            .get::<fields::NativeTextField>(entity)
            .is_some());
        let action = self.action(Command::Field(field), ControlInput::Click);
        self.reduce(&action).unwrap();
        let mut editor = self
            .app
            .world_mut()
            .get_mut::<EditableText>(entity)
            .unwrap();
        editor.queue_edit(TextEdit::SelectAll);
        editor.queue_edit(TextEdit::Insert(value.into()));

        self.app
            .world_mut()
            .run_system_cached(bevy::text::apply_text_edits)
            .unwrap();
        assert_eq!(self.buffer(field), value);
    }

    fn buffer(&self, field: Field) -> String {
        self.app
            .world()
            .get::<EditableText>(self.entity(Command::Field(field)))
            .unwrap()
            .value()
            .to_string()
    }

    fn mouse_click(&mut self, command: Command) -> Vec<Result<Value, String>> {
        let point = self
            .app
            .world()
            .get::<UiGlobalTransform>(self.entity(command))
            .unwrap()
            .affine()
            .translation;
        let event = WindowEvent::MouseButtonInput(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Pressed,
            window: Entity::PLACEHOLDER,
        });
        assert!(!fields::before_window_input(
            self.app.world_mut(),
            &self.handle,
            &event,
            Some(point),
            Modifiers::default()
        )
        .unwrap());
        assert!(self
            .handle
            .pointer(
                PointerPhase::Down,
                [point.x as f64, point.y as f64],
                PointerButton::Primary
            )
            .unwrap());
        assert!(self
            .handle
            .pointer(
                PointerPhase::Up,
                [point.x as f64, point.y as f64],
                PointerButton::Primary
            )
            .unwrap());
        self.handle
            .take_actions()
            .unwrap()
            .iter()
            .map(|action| self.reduce(action))
            .collect()
    }

    fn drain(&mut self) -> Value {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(outcome) = worker::poll(self.app.world_mut(), &self.services) {
                assert_eq!(outcome.operation, "set_body_appearance");
                let value = outcome.value.unwrap();
                assert!(value["render_error"].is_null(), "{value}");
                assert_eq!(value["publication_pending"], false, "{value}");
                return value;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Appearance worker timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[test]
fn unified_material_picker_exposes_metals_and_properties_then_persists_the_same_snapshot() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut panel = Panel::new(&fixture);
    let before = model(&fixture);
    panel.set(Field::Brand, "Generic");
    panel.paint(&fixture);
    let preset = "material.aluminum-6061-t6";
    let state = panel.app.world().resource::<State>();
    let available = choices(state.draft.as_ref().unwrap(), Field::Preset).unwrap();
    assert!(available
        .iter()
        .any(|c| c.value == preset && c.label.starts_with("Metal")));
    assert!(available
        .iter()
        .any(|c| c.value == "generic.pla.gray" && c.label.starts_with("Plastic")));
    panel.set(Field::Preset, preset);
    let expected = limo_cad_export::find_preset(preset)
        .unwrap()
        .to_appearance(BodyId(panel.bodies[0]));
    let details = panel.action(Command::Details, ControlInput::Click);
    panel.reduce(&details).unwrap();
    panel.paint(&fixture);
    let state = panel.app.world().resource::<State>();
    assert!(state.details);
    let control = panel
        .app
        .world()
        .get::<InterfaceControl>(
            state
                .widgets
                .entity("appearance-property-Material category")
                .unwrap(),
        )
        .unwrap();
    assert!(matches!(
        &control.field,
        ControlField::Text {
            read_only: true,
            ..
        }
    ));
    let rows = panel::property_rows(state.draft.as_ref().unwrap());
    assert!(rows
        .iter()
        .any(|(_, label, value)| label.contains("YoungsModulus")
            && value.as_ref().is_some_and(|v| v.contains("68900000000 Pa"))));
    assert!(rows
        .iter()
        .any(|(_, label, value)| label.starts_with("Material source:")
            && value
                .as_ref()
                .is_some_and(|v| v.contains("SHA256") && v.contains("CC-BY"))));
    assert_eq!(
        model(&fixture),
        before,
        "Browsing property details must not mutate the document"
    );
    let details = panel.action(Command::Details, ControlInput::Click);
    panel.reduce(&details).unwrap();
    panel.paint(&fixture);
    let apply = panel.action(Command::Apply, ControlInput::Click);
    panel.reduce(&apply).unwrap();
    panel.drain();
    let saved = appearances(&fixture);
    assert_eq!(
        saved
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["body_id"] == panel.bodies[0])
            .unwrap(),
        &serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn one_mouse_apply_commits_the_visible_buffer_and_undo_redo_preserve_the_exact_project() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut panel = Panel::new(&fixture);
    let before = model(&fixture);
    let before_appearances = appearances(&fixture);
    let before_revision = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap()
        .revision;
    for command in [Command::Apply, Command::Reset] {
        assert!(
            !panel
                .app
                .world()
                .get::<InterfaceControl>(panel.entity(command))
                .unwrap()
                .disabled,
            "A clean form must accept the first click that blurs its uncommitted editor"
        );
    }
    panel.edit_buffer(Field::Color, "#12abEF");
    assert!(!panel
        .app
        .world()
        .resource::<State>()
        .draft
        .as_ref()
        .unwrap()
        .dirty());
    assert_eq!(model(&fixture), before);
    let results = panel.mouse_click(Command::Apply);
    assert_eq!(
        results.len(),
        2,
        "Exactly one blur commit and one Apply are expected"
    );
    for result in results {
        result.unwrap();
    }
    assert!(worker::busy(panel.app.world()));
    panel.drain();
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap()
            .revision,
        before_revision + 1
    );
    let after = model(&fixture);
    let mut expected = before_appearances;
    let appearance = expected
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a["body_id"] == panel.bodies[0])
        .unwrap();
    appearance["color"] = json!({"r":18,"g":171,"b":239,"a":153});
    assert_eq!(
        appearances(&fixture),
        expected,
        "All unedited metadata and the other body's appearance stay exact"
    );
    let mut expected_model: Value = serde_json::from_str(before.as_str().unwrap()).unwrap();
    expected_model["body_appearances"] = expected;
    assert_eq!(
        serde_json::from_str::<Value>(after.as_str().unwrap()).unwrap(),
        expected_model
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(
        model(&fixture),
        before,
        "One shared Undo restores the full pre-edit model"
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(
        model(&fixture),
        after,
        "One shared Redo restores exact appearance metadata"
    );
}

#[test]
fn non_filament_appearance_applies_without_plastic_defaults_and_survives_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut panel = Panel::new(&fixture);
    let before = model(&fixture);
    let mut expected = appearances(&fixture);
    panel.set(Field::FilamentType, "");
    panel.set(Field::Brand, "");
    panel.set(Field::Color, "#41544D");
    let more = panel.action(Command::Scroll(6), ControlInput::Click);
    panel.reduce(&more).unwrap();
    panel.paint(&fixture);
    panel.set(Field::ColorName, "Deep green");
    panel.set(Field::MaterialName, "Painted timber (visual designation)");
    let apply = panel.action(Command::Apply, ControlInput::Click);
    panel.reduce(&apply).unwrap();
    panel.drain();
    panel.paint(&fixture);
    let appearance = expected
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|a| a["body_id"] == panel.bodies[0])
        .unwrap();
    appearance["filament_type"] = json!("");
    appearance["brand"] = json!("");
    appearance["color"] = json!({"r":65,"g":84,"b":77,"a":153});
    appearance["color_name"] = json!("Deep green");
    appearance["material_name"] = json!("Painted timber (visual designation)");
    appearance["filament_id"] = Value::Null;
    appearance["preset_id"] = Value::Null;
    appearance["density_g_cm3"] = Value::Null;
    assert_eq!(appearances(&fixture), expected);
    let mut expected_model: Value = serde_json::from_str(before.as_str().unwrap()).unwrap();
    expected_model["body_appearances"] = expected;
    let after = model(&fixture);
    assert_eq!(
        serde_json::from_str::<Value>(after.as_str().unwrap()).unwrap(),
        expected_model,
        "Applying timber changes only this body's appearance"
    );
    let draft = panel
        .app
        .world()
        .resource::<State>()
        .draft
        .as_ref()
        .unwrap();
    assert!(draft.value.filament_type.is_empty());
    assert!(draft.value.brand.is_empty());
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(model(&fixture), before);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(model(&fixture), after);
}

#[test]
fn invalid_blur_stays_visible_and_custom_cannot_hide_errors_but_a_catalog_choice_replaces_them() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut panel = Panel::new(&fixture);
    let before = model(&fixture);
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    panel.edit_buffer(Field::Color, "#GG0000");
    let results = panel.mouse_click(Command::Apply);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].as_ref().unwrap()["valid"], false);
    assert!(results[1].as_ref().unwrap_err().contains("hexadecimal"));
    assert!(!worker::busy(panel.app.world()));
    panel.paint(&fixture);
    assert_eq!(panel.buffer(Field::Color), "#GG0000");
    assert_eq!(panel.set(Field::FilamentType, "  ")["handled"], true);
    assert_eq!(panel.app.world().resource::<State>().errors.len(), 1);
    panel.set(Field::Preset, "");
    assert_eq!(
        panel.app.world().resource::<State>().errors.len(),
        1,
        "Custom only changes the preset field"
    );
    let apply = panel.action(Command::Apply, ControlInput::Click);
    assert!(panel.reduce(&apply).is_err());
    assert_eq!(model(&fixture), before);
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap(),
        receipt
    );

    let preset = "bambu.pla.basic.red";
    panel.set(Field::Preset, preset);
    assert!(panel.app.world().resource::<State>().errors.is_empty());
    assert_eq!(panel.set(Field::Color, "bad again")["valid"], false);
    panel.set(Field::Preset, preset);
    assert!(panel.app.world().resource::<State>().errors.is_empty());
    let expected = serde_json::to_value(
        limo_cad_export::find_preset(preset)
            .unwrap()
            .to_appearance(BodyId(panel.bodies[0])),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(
            &panel
                .app
                .world()
                .resource::<State>()
                .draft
                .as_ref()
                .unwrap()
                .value
        )
        .unwrap(),
        expected
    );
    panel.paint(&fixture);
    assert_eq!(panel.buffer(Field::Color), "#C82828");
    let results = panel.mouse_click(Command::Apply);
    assert_eq!(results.len(), 1);
    results.into_iter().next().unwrap().unwrap();
    panel.drain();
    assert_eq!(
        appearances(&fixture)
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["body_id"] == panel.bodies[0])
            .unwrap(),
        &expected
    );
}

#[test]
fn one_mouse_reset_discards_even_an_invalid_uncommitted_buffer_without_editing_the_model() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut panel = Panel::new(&fixture);
    let before = model(&fixture);
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    panel.edit_buffer(Field::Color, "not a color");
    let results = panel.mouse_click(Command::Reset);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].as_ref().unwrap()["valid"], false);
    results[1].as_ref().unwrap();
    panel.paint(&fixture);
    assert_eq!(
        panel.buffer(Field::Color),
        saved_appearance(panel.bodies[0]).color.to_hex_rgb()
    );
    assert!(panel.app.world().resource::<State>().errors.is_empty());
    assert!(!panel
        .app
        .world()
        .resource::<State>()
        .draft
        .as_ref()
        .unwrap()
        .dirty());
    assert!(!worker::busy(panel.app.world()));
    assert_eq!(model(&fixture), before);
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap(),
        receipt
    );
}

#[test]
fn stale_body_revision_and_owner_actions_cannot_edit_a_refreshed_appearance_panel() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for change in ["body", "revision", "owner"] {
        let fixture = Fixture::new();
        let mut panel = Panel::new(&fixture);
        let old_owner = fixture.owner();
        let stale_field = panel.action(
            Command::Field(Field::Color),
            ControlInput::SetValue("#FF1234".into()),
        );
        let stale_apply = panel.action(Command::Apply, ControlInput::Click);
        match change {
            "body" => panel.select(&fixture, panel.bodies[1]),
            "revision" => {
                fixture.rename(&old_owner, "Newer revision").unwrap();
            }
            "owner" => {
                fixture
                    .bridge
                    .apply_native_mutation(
                        &fixture.engine,
                        &old_owner,
                        "cad_new_project",
                        &json!({}),
                        || Ok(()),
                    )
                    .unwrap();
                refresh_native_model(&fixture.engine, panel.app.world_mut(), true).unwrap();
            }
            _ => unreachable!(),
        }
        let before = model(&fixture);
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap();
        assert!(
            panel.reduce(&stale_apply).is_err(),
            "Old Apply survived {change}"
        );
        panel.paint(&fixture);
        let fresh = panel
            .app
            .world()
            .resource::<State>()
            .draft
            .as_ref()
            .map(|draft| serde_json::to_value(&draft.value).unwrap());
        assert!(
            panel.reduce(&stale_field).is_err(),
            "Old field targeted the refreshed {change} panel"
        );
        assert!(
            panel.reduce(&stale_apply).is_err(),
            "Old Apply targeted the refreshed {change} panel"
        );
        assert_eq!(
            panel
                .app
                .world()
                .resource::<State>()
                .draft
                .as_ref()
                .map(|draft| serde_json::to_value(&draft.value).unwrap()),
            fresh
        );
        assert!(!worker::busy(panel.app.world()));
        assert_eq!(model(&fixture), before);
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &fixture.owner())
                .unwrap(),
            receipt
        );
    }
}

#[test]
fn pending_appearance_and_errors_survive_unrelated_revisions_and_history_owners() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut panel = Panel::new(&fixture);
    panel.set(Field::Color, "#A12345");
    assert_eq!(panel.set(Field::Color, "#invalid")["valid"], false);
    let pending = {
        let state = panel.app.world().resource::<State>();
        let draft = state.draft.as_ref().unwrap();
        (
            serde_json::to_value(&draft.original).unwrap(),
            serde_json::to_value(&draft.value).unwrap(),
            state.errors.clone(),
        )
    };

    for change in ["rename", "other body", "undo other body"] {
        let stale = panel.action(Command::Apply, ControlInput::Click);
        let generation = panel.app.world().resource::<State>().generation;
        let old_owner = fixture.owner();
        match change {
            "rename" => {
                fixture
                    .rename(&old_owner, "Keep pending appearance")
                    .unwrap();
            }
            "other body" => {
                let mut other = saved_appearance(panel.bodies[1]);
                other.color = Rgba8::opaque(180, 10, 20);
                fixture
                    .bridge
                    .apply_native_mutation(
                        &fixture.engine,
                        &old_owner,
                        "set_body_appearance",
                        &serde_json::to_value(other).unwrap(),
                        || Ok(()),
                    )
                    .unwrap();
            }
            "undo other body" => {
                fixture
                    .bridge
                    .apply_native_history(&fixture.engine, &old_owner, false, || Ok(()))
                    .unwrap();
                let new_owner = fixture.owner();
                assert_eq!(old_owner.window_id, new_owner.window_id);
                assert_eq!(old_owner.document_id, new_owner.document_id);
                assert_ne!(
                    old_owner.epoch, new_owner.epoch,
                    "Undo replaces the owner receipt"
                );
                refresh_native_model(&fixture.engine, panel.app.world_mut(), true).unwrap();
                panel.select(&fixture, panel.bodies[0]);
            }
            _ => unreachable!(),
        }
        panel.paint(&fixture);
        let state = panel.app.world().resource::<State>();
        let draft = state.draft.as_ref().unwrap();
        assert_eq!(
            (
                serde_json::to_value(&draft.original).unwrap(),
                serde_json::to_value(&draft.value).unwrap(),
                state.errors.clone()
            ),
            pending,
            "Draft lost after {change}"
        );
        assert!(state.generation > generation);
        assert_eq!(panel.buffer(Field::Color), "#invalid");
        assert_eq!(panel.buffer(Field::FilamentType), "PETG");
        assert!(
            panel.reduce(&stale).is_err(),
            "Preserving the draft must not revive old controls"
        );
    }

    let before = model(&fixture);
    panel.set(Field::Color, "#A12345");
    let apply = panel.action(Command::Apply, ControlInput::Click);
    panel.reduce(&apply).unwrap();
    panel.drain();
    let saved = appearances(&fixture);
    let saved = saved
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["body_id"] == panel.bodies[0])
        .unwrap();
    assert_eq!(saved, &pending.1);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(
        model(&fixture),
        before,
        "Undo preserves the unrelated rename and metadata exactly"
    );
}

#[test]
fn exact_canonical_changes_history_and_selection_replace_a_pending_appearance() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut panel = Panel::new(&fixture);
    let original = saved_appearance(panel.bodies[0]);
    let mut changed = original.clone();
    changed.diameter_mm += 1e-10;
    assert_eq!(
        original, changed,
        "The shared equality deliberately tolerates this difference"
    );
    assert_ne!(
        serde_json::to_value(&original).unwrap(),
        serde_json::to_value(&changed).unwrap()
    );

    for change in ["canonical", "undo canonical", "selection"] {
        panel.set(Field::Color, "#FF0000");
        panel.set(Field::Color, "#invalid");
        let stale = panel.action(Command::Apply, ControlInput::Click);
        let expected = match change {
            "canonical" => {
                fixture
                    .bridge
                    .apply_native_mutation(
                        &fixture.engine,
                        &fixture.owner(),
                        "set_body_appearance",
                        &serde_json::to_value(&changed).unwrap(),
                        || Ok(()),
                    )
                    .unwrap();
                changed.clone()
            }
            "undo canonical" => {
                fixture
                    .bridge
                    .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
                    .unwrap();
                refresh_native_model(&fixture.engine, panel.app.world_mut(), true).unwrap();
                panel.select(&fixture, panel.bodies[0]);
                original.clone()
            }
            "selection" => {
                panel.select(&fixture, panel.bodies[1]);
                saved_appearance(panel.bodies[1])
            }
            _ => unreachable!(),
        };
        panel.paint(&fixture);
        let state = panel.app.world().resource::<State>();
        let draft = state.draft.as_ref().unwrap();
        assert_eq!(
            serde_json::to_value(&draft.original).unwrap(),
            serde_json::to_value(&expected).unwrap(),
            "Wrong canonical source after {change}"
        );
        assert_eq!(
            serde_json::to_value(&draft.value).unwrap(),
            serde_json::to_value(&expected).unwrap(),
            "Pending value survived {change}"
        );
        assert!(state.errors.is_empty());
        assert_eq!(panel.buffer(Field::Color), expected.color.to_hex_rgb());
        assert!(panel.reduce(&stale).is_err());
        assert!(!worker::busy(panel.app.world()));
    }
}

#[test]
fn shared_slicer_publications_repaint_and_interactions_use_the_latest_persisted_target() {
    use limo_cad_export::SlicerTarget;
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut panel = Panel::new(&fixture);
    let before = model(&fixture);
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    let path = crate::session_bridge::session_root().join("body-slicer-target.json");
    let mut other_window = preferences::Observer::at(path.clone());
    assert_eq!(
        panel.app.world().resource::<State>().slicer_target,
        SlicerTarget::Standard
    );
    other_window.write(SlicerTarget::Cura).unwrap();
    panel.paint(&fixture);
    assert_eq!(
        panel.app.world().resource::<State>().slicer_target,
        SlicerTarget::Cura
    );
    assert!(caption(panel.app.world())
        .unwrap()
        .contains("UltiMaker Cura"));
    let control = panel
        .app
        .world()
        .get::<InterfaceControl>(panel.entity(Command::SlicerTarget))
        .unwrap();
    assert!(matches!(&control.field, ControlField::Choice {value, ..} if value == "cura"));

    std::fs::write(
        &path,
        serde_json::to_vec(&SlicerTarget::PrusaSlicer).unwrap(),
    )
    .unwrap();
    let choose = panel.action(Command::SlicerTarget, ControlInput::Click);
    assert_eq!(panel.reduce(&choose).unwrap()["slicer_target"], "cura");
    assert_eq!(
        serde_json::from_slice::<SlicerTarget>(&std::fs::read(&path).unwrap()).unwrap(),
        SlicerTarget::Cura
    );

    std::fs::write(&path, br#""broken""#).unwrap();
    panel.set(Field::Color, "#234567");
    assert!(panel
        .app
        .world()
        .resource::<State>()
        .preference_error
        .is_some());
    std::fs::write(
        &path,
        serde_json::to_vec(&SlicerTarget::OrcaSlicer).unwrap(),
    )
    .unwrap();
    panel.set(Field::Color, "#345678");
    let state = panel.app.world().resource::<State>();
    assert_eq!(state.slicer_target, SlicerTarget::Standard);
    assert!(state.preference_error.is_none());
    assert_eq!(
        model(&fixture),
        before,
        "Application preferences and pending fields do not mutate the project"
    );
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap(),
        receipt
    );
    assert!(!worker::busy(panel.app.world()));
}
