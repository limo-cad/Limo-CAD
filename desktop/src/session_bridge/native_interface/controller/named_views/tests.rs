use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

/// Production controls and reducers with explicit headless layout output.
/// Native captures separately verify font shaping and the visible result.
struct ViewControls {
    app: App,
    services: NativeServices,
    handle: NativeInterfaceHandle,
    owner: DocumentContext,
    camera: Entity,
}

impl ViewControls {
    fn new(fixture: &Fixture) -> Self {
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
            .init_resource::<ViewportUiAssets>()
            .init_resource::<Time<bevy::time::Real>>()
            .init_resource::<UiScale>()
            .init_resource::<bevy::input_focus::InputFocus>()
            .insert_resource(handle.clone());
        interface_shell::fields::install(&mut app);
        let camera = app.world_mut().spawn(InterfaceCamera).id();
        let bodies = refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap();
        app.insert_resource(NativeRenderedDocument {
            owner: receipt.owner.clone(),
            revision: receipt.revision,
            bodies,
        });
        worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
        let mut result = Self {
            app,
            services,
            handle,
            owner: receipt.owner,
            camera,
        };
        result.paint(1360., 860.);
        if !read_views(&fixture.engine).unwrap().views.is_empty() {
            result
                .invoke("Expand Named Views", ControlInput::Click)
                .unwrap();
            result.paint(1360., 860.);
        }
        result
    }

    fn paint(&mut self, width: f32, height: f32) {
        use bevy::ui::{ComputedStackIndex, ComputedUiRenderTargetInfo, UiGlobalTransform};
        let world = self.app.world_mut();
        let revision = self
            .services
            .bridge
            .native_document_receipt(&self.services.engine, &self.owner)
            .unwrap()
            .revision;
        browser::synchronize(
            world,
            &self.services,
            &self.owner,
            revision,
            InterfaceRect {
                x: 0.,
                y: 120.,
                width: 232.,
                height: f64::from(height) - 160.,
            },
            &mut 0.,
        )
        .unwrap();
        synchronize(
            world,
            self.camera,
            &self.services,
            &self.owner,
            width,
            height,
        )
        .unwrap();
        let client = InterfaceRect {
            x: 0.,
            y: 0.,
            width: width.into(),
            height: height.into(),
        };
        self.handle
            .present(InterfaceFrame {
                context: self.owner.clone(),
                client,
                surface: client,
                canvases: vec![],
                surfaces: ["solid/selection", "document/views"]
                    .map(|name| limo_cad_interface::Surface {
                        name: name.into(),
                        text: None,
                    })
                    .into(),
                modal_stack: vec![],
                document_visible: true,
            })
            .unwrap();
        let px = |value| match value {
            Val::Px(value) => value,
            _ => panic!("absolute control bounds required"),
        };
        let layout: Vec<_> = world
            .query_filtered::<(Entity, &Node, &ZIndex), With<InterfaceControl>>()
            .iter(world)
            .map(|(entity, node, z)| {
                let size = Vec2::new(px(node.width), px(node.height));
                (
                    entity,
                    size,
                    Vec2::new(px(node.left), px(node.top)) + size / 2.,
                    z.0,
                )
            })
            .collect();
        for (entity, size, center, z) in layout {
            world.entity_mut(entity).insert((
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

    fn entity(&mut self, label: &str) -> Entity {
        self.app
            .world_mut()
            .query::<(Entity, &InterfaceControl)>()
            .iter(self.app.world())
            .find(|(_, control)| control.label == label)
            .unwrap_or_else(|| panic!("Missing {label}"))
            .0
    }

    fn action(&mut self, label: &str, input: ControlInput) -> NativeInterfaceAction {
        let entity = self.entity(label);
        self.handle
            .resolve_input(
                limo_cad_interface::ControlKey(entity.to_bits()),
                input,
                &self.owner,
            )
            .unwrap()
    }

    fn type_in_buffer(&mut self, label: &str, value: &str) {
        use bevy::text::{EditableText, TextEdit};
        self.invoke(label, ControlInput::Click).unwrap();
        let entity = self.entity(label);
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
    }

    fn invoke(&mut self, label: &str, input: ControlInput) -> Result<Value, String> {
        let action = self.action(label, input);
        super::super::reduce_control_input(
            &self.services.engine,
            &self.services.bridge,
            self.app.world_mut(),
            &self.handle,
            &action,
        )
    }

    fn outcome(&mut self) -> Result<Value, String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(outcome) = worker::poll(self.app.world_mut(), &self.services) {
                return outcome.value;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Saved-view worker timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn drain(&mut self) {
        let value = self.outcome().unwrap();
        assert!(value["render_error"].is_null(), "{value}");
    }
}

#[test]
fn browser_names_activate_saved_views_by_pointer_and_keyboard_without_opening_the_editor() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut ui = ViewControls::new(&fixture);
        let geometry = fixture.engine.viewport_frame();
        for input in [
            ControlInput::Click,
            ControlInput::DoubleClick,
            ControlInput::Key(limo_cad_interface::KeyChord::plain("Enter")),
            ControlInput::Key(limo_cad_interface::KeyChord::plain("Space")),
        ] {
            native_viewport::apply_interface_view(
                ui.app.world_mut(),
                &ui.owner.document_id,
                Some(native_viewport::ViewportCamera::default()),
                None,
            )
            .unwrap();
            ui.paint(1360., 860.);
            assert_eq!(
                ui.invoke(&saved.name, input).unwrap()["mutation_pending"],
                true
            );
            ui.drain();
            let camera = native_viewport::interface_view(ui.app.world()).1;
            assert_eq!(camera.position, saved.camera.position.map(|v| v as f32));
            assert_eq!(camera.target, saved.camera.target.map(|v| v as f32));
            assert_eq!(camera.up, saved.camera.up.map(|v| v as f32));
            assert_eq!(
                read_views(&fixture.engine).unwrap().active.as_deref(),
                Some(saved.name.as_str())
            );
            assert!(
                !active(ui.app.world()),
                "Activating a browser row must not open the editor"
            );
            assert!(Arc::ptr_eq(
                &geometry.document,
                &fixture.engine.viewport_frame().document
            ));
        }
        ui.paint(1360., 860.);
        let camera = native_viewport::interface_view(ui.app.world()).1;
        ui.invoke(
            &format!("Edit saved view {}", saved.name),
            ControlInput::Click,
        )
        .unwrap();
        assert!(active(ui.app.world()));
        assert_eq!(native_viewport::interface_view(ui.app.world()).1, camera);
    });
}

#[test]
fn an_in_flight_double_click_recalls_once_without_a_second_pending_operation() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut ui = ViewControls::new(&fixture);
        let before = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &ui.owner)
            .unwrap();
        let first = ui.invoke(&saved.name, ControlInput::Click).unwrap();
        assert_eq!(first["mutation_pending"], true);
        let continuation = ui.action(&saved.name, ControlInput::DoubleClick);
        assert!(coalesces_native_double_click(ui.app.world(), &continuation));
        let second = ui.invoke(&saved.name, ControlInput::DoubleClick).unwrap();
        assert_eq!(second["mutation_id"], first["mutation_id"]);
        ui.drain();
        assert!(!worker::busy(ui.app.world()));
        assert!(!ui.app.world().contains_resource::<PendingRecall>());
        ui.paint(1360., 860.);
        assert_eq!(
            ui.invoke(&saved.name, ControlInput::DoubleClick).unwrap()["already_active"],
            true
        );
        assert!(!worker::busy(ui.app.world()));
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &ui.owner)
                .unwrap()
                .revision,
            before.revision + 1
        );
        assert_eq!(
            read_views(&fixture.engine).unwrap().active.as_deref(),
            Some(saved.name.as_str())
        );
    });
}

#[test]
fn reopening_and_creating_clean_unsaved_views_recaptures_the_current_camera() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let mut ui = ViewControls::new(&fixture);
        open(ui.app.world_mut(), &fixture.engine, &ui.owner, None).unwrap();
        ui.paint(1360., 860.);
        ui.invoke("Close named views", ControlInput::Click).unwrap();
        for name in [None, Some("")] {
            let mut camera = native_viewport::interface_view(ui.app.world()).1;
            camera.position[0] += 80.;
            native_viewport::apply_interface_view(
                ui.app.world_mut(),
                &ui.owner.document_id,
                Some(camera),
                None,
            )
            .unwrap();
            open(ui.app.world_mut(), &fixture.engine, &ui.owner, name).unwrap();
            let state = ui.app.world().resource::<State>();
            assert_eq!(
                state.draft.as_ref().unwrap().camera.position,
                camera.position.map(f64::from)
            );
            assert!(!has_unsaved_changes(state));
            assert!(state.selected.is_none());
        }
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::Name,
            "Keep unfinished work",
            UnitSystem::Mm,
        )
        .unwrap();
        let draft = ui.app.world().resource::<State>().draft.clone();
        ui.paint(1360., 860.);
        ui.invoke("Close named views", ControlInput::Click).unwrap();
        open(ui.app.world_mut(), &fixture.engine, &ui.owner, None).unwrap();
        assert_eq!(ui.app.world().resource::<State>().draft, draft);
        assert!(open(ui.app.world_mut(), &fixture.engine, &ui.owner, Some("")).is_err());
        assert_eq!(ui.app.world().resource::<State>().draft, draft);
    });
}

#[test]
fn reopening_a_clean_editor_uses_the_active_engine_view() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut second = saved.clone();
        second.id = None;
        second.name = "Externally activated view".into();
        second.camera.position[0] += 30.;
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                "upsert_named_view",
                &serde_json::to_value(&second).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let mut ui = ViewControls::new(&fixture);
        open(
            ui.app.world_mut(),
            &fixture.engine,
            &ui.owner,
            Some(&saved.name),
        )
        .unwrap();
        ui.paint(1360., 860.);
        ui.invoke("Close named views", ControlInput::Click).unwrap();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &ui.owner,
                "recall_named_view",
                &json!({"name":second.name}),
                || Ok(()),
            )
            .unwrap();
        open(ui.app.world_mut(), &fixture.engine, &ui.owner, None).unwrap();
        let state = ui.app.world().resource::<State>();
        assert_eq!(state.selected.as_deref(), Some(second.name.as_str()));
        assert_eq!(
            state.draft.as_ref(),
            Some(&stored_view(&fixture, &second.name))
        );
        assert!(!has_unsaved_changes(state));
    });
}

#[test]
fn recall_completion_preserves_edits_made_after_enqueue_and_the_next_paint() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut second = saved.clone();
        second.id = None;
        second.name = "Recall target".into();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                "upsert_named_view",
                &serde_json::to_value(&second).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let mut ui = ViewControls::new(&fixture);
        open(
            ui.app.world_mut(),
            &fixture.engine,
            &ui.owner,
            Some(&saved.name),
        )
        .unwrap();
        activate(
            ui.app.world_mut(),
            &fixture.engine,
            &fixture.bridge,
            &ui.owner,
            &second.name,
        )
        .unwrap();
        // A direct SetValue can reach the reducer while the worker is pending.
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::Name,
            "New draft edit",
            UnitSystem::Mm,
        )
        .unwrap();
        let draft = ui.app.world().resource::<State>().draft.clone();
        ui.drain();
        ui.paint(1360., 860.);
        let state = ui.app.world().resource::<State>();
        assert_eq!(state.draft, draft);
        assert_eq!(state.selected.as_deref(), Some(saved.name.as_str()));
        assert!(has_unsaved_changes(state));
        assert_eq!(
            read_views(&fixture.engine).unwrap().active.as_deref(),
            Some(second.name.as_str())
        );
    });
}

#[test]
fn recall_completion_refreshes_a_hidden_editors_stale_view_list() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut ui = ViewControls::new(&fixture);
        open(
            ui.app.world_mut(),
            &fixture.engine,
            &ui.owner,
            Some(&saved.name),
        )
        .unwrap();
        ui.paint(1360., 860.);
        ui.invoke("Close named views", ControlInput::Click).unwrap();
        let mut added = saved.clone();
        added.id = None;
        added.name = "Newly saved view".into();
        added.camera.target[0] += 15.;
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &ui.owner,
                "upsert_named_view",
                &serde_json::to_value(&added).unwrap(),
                || Ok(()),
            )
            .unwrap();
        assert!(!ui
            .app
            .world()
            .resource::<State>()
            .views
            .iter()
            .any(|v| v.name == added.name));
        activate(
            ui.app.world_mut(),
            &fixture.engine,
            &fixture.bridge,
            &ui.owner,
            &added.name,
        )
        .unwrap();
        ui.drain();
        let state = ui.app.world().resource::<State>();
        assert!(!state.visible);
        assert_eq!(state.selected.as_deref(), Some(added.name.as_str()));
        assert_eq!(
            state.draft.as_ref(),
            Some(&stored_view(&fixture, &added.name))
        );
        assert_eq!(state.views, read_views(&fixture.engine).unwrap().views);
    });
}

#[test]
fn clean_drafts_reconcile_external_changes_after_discard_or_manually_reverting_edits() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        for operation in ["upsert_named_view", "delete_named_view"] {
            for discard in [true, false] {
                let fixture = Fixture::new();
                let saved = solid_and_view(&fixture);
                let mut ui = ViewControls::new(&fixture);
                open(
                    ui.app.world_mut(),
                    &fixture.engine,
                    &ui.owner,
                    Some(&saved.name),
                )
                .unwrap();
                ui.paint(1360., 860.);
                edit(
                    ui.app.world_mut(),
                    &fixture.engine,
                    Field::Name,
                    "Keep my edit",
                    UnitSystem::Mm,
                )
                .unwrap();
                let draft = ui.app.world().resource::<State>().draft.clone();
                let mut updated = saved.clone();
                updated.camera.position[0] += 25.;
                let arguments = if operation == "upsert_named_view" {
                    serde_json::to_value(&updated).unwrap()
                } else {
                    json!({"name":saved.name})
                };
                fixture
                    .bridge
                    .apply_native_mutation(
                        &fixture.engine,
                        &ui.owner,
                        operation,
                        &arguments,
                        || Ok(()),
                    )
                    .unwrap();
                ui.paint(1360., 860.);
                assert_eq!(ui.app.world().resource::<State>().draft, draft);
                if discard {
                    ui.invoke("Discard edits", ControlInput::Click).unwrap();
                } else {
                    edit(
                        ui.app.world_mut(),
                        &fixture.engine,
                        Field::Name,
                        &saved.name,
                        UnitSystem::Mm,
                    )
                    .unwrap();
                }
                ui.paint(1360., 860.);
                let state = ui.app.world().resource::<State>();
                assert!(state.visible);
                assert!(!has_unsaved_changes(state));
                if operation == "upsert_named_view" {
                    assert_eq!(state.selected.as_deref(), Some(updated.name.as_str()));
                    assert_eq!(state.draft.as_ref(), Some(&updated));
                    assert_eq!(state.original.as_ref(), Some(&updated));
                } else {
                    assert!(state.selected.is_none());
                    assert!(state.draft.is_none());
                    assert!(state.draft_baseline.is_none());
                    assert!(saved_action_error(state, &Command::Recall).is_some());
                }
            }
        }
    });
}

#[test]
fn successful_save_acknowledges_only_the_submitted_draft() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let mut ui = ViewControls::new(&fixture);
        open(ui.app.world_mut(), &fixture.engine, &ui.owner, None).unwrap();
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::Name,
            "Saved draft",
            UnitSystem::Mm,
        )
        .unwrap();
        ui.app.world_mut().resource_mut::<State>().scroll = usize::MAX;
        ui.paint(1360., 860.);
        ui.invoke("Save view", ControlInput::Click).unwrap();
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::PrintLayout,
            "true",
            UnitSystem::Mm,
        )
        .unwrap();
        ui.drain();
        ui.paint(1360., 860.);
        let expected = stored_view(&fixture, "Saved draft");
        assert_eq!(
            ui.app
                .world()
                .resource::<State>()
                .draft
                .as_ref()
                .unwrap()
                .id,
            expected.id
        );
        assert!(has_unsaved_changes(ui.app.world().resource::<State>()));
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::PrintLayout,
            "false",
            UnitSystem::Mm,
        )
        .unwrap();
        ui.paint(1360., 860.);
        assert!(!has_unsaved_changes(ui.app.world().resource::<State>()));
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::PrintLayout,
            "true",
            UnitSystem::Mm,
        )
        .unwrap();
        ui.app.world_mut().resource_mut::<State>().scroll = usize::MAX;
        ui.paint(1360., 860.);
        ui.invoke("Save view", ControlInput::Click).unwrap();
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::PrintLayout,
            "false",
            UnitSystem::Mm,
        )
        .unwrap();
        ui.drain();
        ui.paint(1360., 860.);
        let state = ui.app.world().resource::<State>();
        assert!(has_unsaved_changes(state));
        assert!(!state.draft.as_ref().unwrap().print_layout);
        assert!(state.draft_baseline.as_ref().unwrap().print_layout);
        assert!(read_views(&fixture.engine).unwrap().views[0].print_layout);
    });
}

#[test]
fn discarding_a_preview_restores_the_camera_and_keeps_the_editor_open() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut ui = ViewControls::new(&fixture);
        open(
            ui.app.world_mut(),
            &fixture.engine,
            &ui.owner,
            Some(&saved.name),
        )
        .unwrap();
        ui.paint(1360., 860.);
        let (_, camera, presentation, _) = native_viewport::interface_view_snapshot(ui.app.world());
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &ui.owner)
            .unwrap();
        let mut preview_camera = camera;
        preview_camera.position[0] += 90.;
        native_viewport::apply_interface_view(
            ui.app.world_mut(),
            &ui.owner.document_id,
            Some(preview_camera),
            None,
        )
        .unwrap();
        {
            let mut state = ui.app.world_mut().resource_mut::<State>();
            state.previewing = true;
            state.preview_origin = Some(PreviewOrigin {
                receipt,
                camera,
                presentation,
            });
        }
        ui.paint(1360., 860.);
        ui.invoke("Discard edits", ControlInput::Click).unwrap();
        let state = ui.app.world().resource::<State>();
        assert!(state.visible);
        assert!(!has_unsaved_changes(state));
        assert!(state.preview_origin.is_none());
        assert_eq!(state.draft.as_ref(), Some(&saved));
        assert_eq!(native_viewport::interface_view(ui.app.world()).1, camera);
    });
}

#[test]
fn activation_and_discard_accessible_names_match_the_localized_captions() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut ui = ViewControls::new(&fixture);
        for locale in crate::app_preferences::locale::SUPPORTED {
            native_viewport::localization::set_locale(ui.app.world_mut(), locale);
            open(
                ui.app.world_mut(),
                &fixture.engine,
                &ui.owner,
                Some(&saved.name),
            )
            .unwrap();
            edit(
                ui.app.world_mut(),
                &fixture.engine,
                Field::Name,
                "Unsaved edit",
                UnitSystem::Mm,
            )
            .unwrap();
            ui.paint(1360., 860.);
            for key in ["namedViews.activate", "namedViews.discard"] {
                let label =
                    native_viewport::localization::translate(ui.app.world(), key).to_owned();
                let entity = ui.entity(&label);
                assert_eq!(
                    ui.app
                        .world()
                        .get::<interface_shell::InterfaceCaption>(entity)
                        .unwrap()
                        .0,
                    label
                );
            }
            let discard =
                native_viewport::localization::translate(ui.app.world(), "namedViews.discard")
                    .to_owned();
            ui.invoke(&discard, ControlInput::Click).unwrap();
            let state = ui.app.world().resource::<State>();
            assert!(state.visible, "Discard must keep the editor open");
            assert!(!has_unsaved_changes(state));
            assert_eq!(state.draft.as_ref(), Some(&saved));
        }
    });
}

#[test]
fn browser_activation_and_editor_selection_preserve_unfinished_view_edits() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut second = saved.clone();
        second.id = None;
        second.name = "Second view".into();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                "upsert_named_view",
                &serde_json::to_value(&second).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let mut ui = ViewControls::new(&fixture);
        ui.invoke(
            &format!("Edit saved view {}", saved.name),
            ControlInput::Click,
        )
        .unwrap();
        ui.paint(1360., 860.);
        ui.type_in_buffer("View name", "Unfinished edits");
        let before = read_views(&fixture.engine).unwrap();
        // The browser click must flush the real text buffer before deciding
        // whether switching would discard an edit.
        assert!(ui
            .invoke(&second.name, ControlInput::Click)
            .unwrap_err()
            .contains("Save or discard"));
        let draft = ui.app.world().resource::<State>().draft.clone();
        assert_eq!(draft.as_ref().unwrap().name, "Unfinished edits");
        ui.paint(1360., 860.);
        for label in [&second.name, &format!("Edit saved view {}", second.name)] {
            assert!(ui
                .invoke(label, ControlInput::Click)
                .unwrap_err()
                .contains("Save or discard"));
            assert_eq!(ui.app.world().resource::<State>().draft, draft);
            assert_eq!(read_views(&fixture.engine).unwrap(), before);
        }
        assert!(edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::Saved,
            &second.name,
            UnitSystem::Mm
        )
        .is_err());
        assert_eq!(ui.app.world().resource::<State>().draft, draft);
        ui.invoke("Discard edits", ControlInput::Click).unwrap();
        ui.paint(1360., 860.);
        ui.invoke(&second.name, ControlInput::Click).unwrap();
        ui.drain();
        assert_eq!(
            read_views(&fixture.engine).unwrap().active.as_deref(),
            Some(second.name.as_str())
        );
        assert_eq!(
            ui.app.world().resource::<State>().selected.as_deref(),
            Some(second.name.as_str())
        );

        open(ui.app.world_mut(), &fixture.engine, &ui.owner, Some("")).unwrap();
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::Name,
            "New unsaved view",
            UnitSystem::Mm,
        )
        .unwrap();
        assert!(activate(
            ui.app.world_mut(),
            &fixture.engine,
            &fixture.bridge,
            &ui.owner,
            &saved.name
        )
        .is_err());
        assert_eq!(
            ui.app
                .world()
                .resource::<State>()
                .draft
                .as_ref()
                .unwrap()
                .name,
            "New unsaved view"
        );
    });
}

#[test]
fn activate_view_stays_visible_on_every_editor_page_at_compact_sizes() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut ui = ViewControls::new(&fixture);
        open(
            ui.app.world_mut(),
            &fixture.engine,
            &ui.owner,
            Some(&saved.name),
        )
        .unwrap();
        for (width, height) in [(1360., 860.), (900., 500.)] {
            let mut pinned = None;
            for scroll in [0, 5, usize::MAX] {
                ui.app.world_mut().resource_mut::<State>().scroll = scroll;
                ui.paint(width, height);
                let world = ui.app.world_mut();
                let buttons: Vec<_> = world
                    .query::<(&InterfaceControl, &Node)>()
                    .iter(world)
                    .filter(|(control, _)| control.label == "Activate view")
                    .map(|(control, node)| (control.disabled, node.clone()))
                    .collect();
                assert_eq!(
                    buttons.len(),
                    1,
                    "Exactly one activation entry in the editor"
                );
                assert!(!buttons[0].0);
                if let Some(pinned) = &pinned {
                    assert_eq!(&buttons[0].1, pinned);
                }
                pinned = Some(buttons[0].1.clone());
            }
        }
    });
}

#[test]
fn a_saved_view_click_cannot_follow_document_replacement() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut ui = ViewControls::new(&fixture);
        open(
            ui.app.world_mut(),
            &fixture.engine,
            &ui.owner,
            Some(&saved.name),
        )
        .unwrap();
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::Name,
            "Previous document draft",
            UnitSystem::Mm,
        )
        .unwrap();
        assert!(has_unsaved_changes(ui.app.world().resource::<State>()));
        let stale = ui.action(&saved.name, ControlInput::Click);
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &ui.owner,
                "cad_new_project",
                &json!({}),
                || Ok(()),
            )
            .unwrap();
        assert!(super::super::reduce_control_input(
            &fixture.engine,
            &fixture.bridge,
            ui.app.world_mut(),
            &ui.handle,
            &stale
        )
        .is_err());
        assert!(!worker::busy(ui.app.world()));
        assert!(read_views(&fixture.engine).unwrap().views.is_empty());
        ui.owner = fixture.owner();
        ui.paint(1360., 860.);
        assert!(!has_unsaved_changes(ui.app.world().resource::<State>()));
        open(ui.app.world_mut(), &fixture.engine, &ui.owner, None).unwrap();
        assert!(
            !has_unsaved_changes(ui.app.world().resource::<State>()),
            "An untouched new draft can be replaced without losing an edit"
        );
    });
}

#[test]
fn deleting_a_view_clears_its_draft_baseline_before_another_activation() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut second = saved.clone();
        second.id = None;
        second.name = "Remaining view".into();
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                "upsert_named_view",
                &serde_json::to_value(&second).unwrap(),
                || Ok(()),
            )
            .unwrap();
        let mut ui = ViewControls::new(&fixture);
        open(
            ui.app.world_mut(),
            &fixture.engine,
            &ui.owner,
            Some(&saved.name),
        )
        .unwrap();
        let occurrence = format!("occurrence:{}", saved.occurrence_offsets[0].occurrence_id.0);
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::Target,
            &occurrence,
            UnitSystem::Mm,
        )
        .unwrap();
        edit(
            ui.app.world_mut(),
            &fixture.engine,
            Field::Translation(0),
            "unfinished+",
            UnitSystem::Mm,
        )
        .unwrap();
        ui.app.world_mut().resource_mut::<State>().scroll = usize::MAX;
        ui.paint(1360., 860.);
        ui.invoke("Delete saved view", ControlInput::Click).unwrap();
        ui.drain();
        assert!(!has_unsaved_changes(ui.app.world().resource::<State>()));
        assert!(ui.app.world().resource::<State>().draft_baseline.is_none());
        ui.paint(1360., 860.);
        ui.invoke(&second.name, ControlInput::Click).unwrap();
        ui.drain();
        assert_eq!(
            read_views(&fixture.engine).unwrap().active.as_deref(),
            Some(second.name.as_str())
        );
        assert!(!read_views(&fixture.engine)
            .unwrap()
            .views
            .iter()
            .any(|view| view.name == saved.name));
    });
}

#[test]
fn failed_async_recall_preserves_the_camera_and_editor() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let saved = solid_and_view(&fixture);
        let mut ui = ViewControls::new(&fixture);
        open(
            ui.app.world_mut(),
            &fixture.engine,
            &ui.owner,
            Some(&saved.name),
        )
        .unwrap();
        let camera = native_viewport::interface_view(ui.app.world()).1;
        let before = read_views(&fixture.engine).unwrap();
        let draft = ui.app.world().resource::<State>().draft.clone();
        activate(
            ui.app.world_mut(),
            &fixture.engine,
            &fixture.bridge,
            &ui.owner,
            "Missing view",
        )
        .unwrap();
        assert!(ui.outcome().is_err());
        assert_eq!(native_viewport::interface_view(ui.app.world()).1, camera);
        assert_eq!(read_views(&fixture.engine).unwrap(), before);
        assert_eq!(
            ui.app.world().resource::<State>().draft.as_ref(),
            draft.as_ref()
        );
    });
}

#[test]
fn print_metadata_receipts_preserve_unapplied_layout_preview_and_its_export_guard() {
    let mut world = World::new();
    let owner = DocumentContext {
        window_id: "main".into(),
        document_id: "owned-document".into(),
        epoch: 7,
    };
    let draft = view();
    world.insert_resource(State {
        owner: Some(owner.clone()),
        revision: Some(8),
        visible: true,
        previewing: true,
        draft: Some(draft.clone()),
        ..default()
    });
    advance_metadata(&mut world, &owner, 9);
    let state = world.resource::<State>();
    assert_eq!(state.revision, Some(9));
    assert_eq!(state.draft.as_ref(), Some(&draft));
    assert!(state.previewing);
    assert!(ensure_exportable(&world).is_err());
    advance_metadata(&mut world, &DocumentContext { epoch: 8, ..owner }, 10);
    assert_eq!(
        world.resource::<State>().revision,
        Some(9),
        "Another document incarnation cannot advance this preview"
    );
}

#[test]
fn saved_named_view_edits_use_the_shared_snapshot_undo_stack() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut view = solid_and_view(&fixture);
    let model =
        || parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let original = model();
    view.part_offsets.push(ViewPartOffsetDto {
        body_id: view.visible_body_ids[0],
        translation: [21., 0., 0.],
    });
    for (operation, args) in [
        ("upsert_named_view", serde_json::to_value(&view).unwrap()),
        (
            "rename_named_view",
            json!({"name":view.name,"new_name":"Renamed fixture"}),
        ),
        ("delete_named_view", json!({"name":"Renamed fixture"})),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &fixture.owner(), operation, &args, || {
                Ok(())
            })
            .unwrap();
    }
    let deleted = model();
    for _ in 0..3 {
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
            .unwrap();
    }
    assert_eq!(
        model(),
        original,
        "Undo must restore view placement and name without changing source definitions"
    );
    for _ in 0..3 {
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
            .unwrap();
    }
    assert_eq!(model(), deleted);
}

fn view() -> NamedViewConfigurationDto {
    NamedViewConfigurationDto {
        id: None,
        name: "Fixture view".into(),
        camera: ViewCameraDto {
            position: [100., -100., 100.],
            target: [0.; 3],
            up: [0., 0., 1.],
        },
        visible_body_ids: vec![7],
        part_offsets: vec![],
        occurrence_offsets: vec![ViewOccurrenceOffsetDto {
            occurrence_id: limo_cad_sketch::OccurrenceId(42),
            translation: [10., 20., 30.],
            rotation: [
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
        }],
        print_layout: true,
        print_bed: PrintBedDto::default(),
    }
}

fn stored_view(fixture: &Fixture, name: &str) -> NamedViewConfigurationDto {
    read_views(&fixture.engine)
        .unwrap()
        .views
        .into_iter()
        .find(|view| view.name == name)
        .unwrap()
}

fn solid_and_view(fixture: &Fixture) -> NamedViewConfigurationDto {
    for (operation, arguments) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":6.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3.}}),
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
    let assembly = read_assembly(&fixture.engine).unwrap();
    let mut saved = view();
    saved.visible_body_ids = fixture
        .engine
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .map(|b| b.id.0)
        .collect();
    saved.occurrence_offsets[0].occurrence_id = assembly.component_structure.occurrences[0].id;
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "upsert_named_view",
            &serde_json::to_value(&saved).unwrap(),
            || Ok(()),
        )
        .unwrap();
    stored_view(fixture, &saved.name)
}

#[test]
fn applied_inbox_named_view_recall_updates_camera_without_moving_source_geometry() {
    crate::session_bridge::tests::with_isolated_session_test(applied_inbox_named_view_recall_case);
}

fn applied_inbox_named_view_recall_case() {
    let fixture = Fixture::new();
    let saved = solid_and_view(&fixture);
    let source = serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "project_set_visibility",
            &json!({"hidden_body_ids":saved.visible_body_ids}),
            || Ok(()),
        )
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    let bodies = refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let (document, mut camera, mut presentation, _) =
        native_viewport::interface_view_snapshot(app.world());
    camera.position = [14., -20., 60.];
    camera.target = [1., 2., 3.];
    camera.up = [0., 1., 0.];
    presentation.selected_surface_point = Some(limo_cad_solid::Point3Dto {
        x: 3.,
        y: 2.,
        z: 3.,
    });
    native_viewport::apply_interface_view(
        app.world_mut(),
        &document,
        Some(camera),
        Some(presentation),
    )
    .unwrap();
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &fixture.owner(), "solid")
        .unwrap();
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let revision = fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    let owner = fixture.owner();
    app.world_mut().insert_resource(NativeRenderedDocument {
        owner: owner.clone(),
        revision,
        bodies,
    });
    let inbox = crate::session_bridge::session_root()
        .join(&session)
        .join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::write(
        inbox.join("1.json"),
        json!({"name":"recall_named_view","arguments":{"name":saved.name},
            "base_generation":revision,"session_id":session,"window_id":"main",
            "document_id":owner.document_id})
        .to_string(),
    )
    .unwrap();
    let applied = crate::session_bridge::apply_or_reject_one_inbox_op_with_presentation_guard(
        &fixture.bridge,
        "main",
        &fixture.engine,
        None,
        Some((&owner.document_id, &session)),
        false,
    )
    .unwrap();
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["model_changed"], true);
    let recalled: limo_cad_sketch::RecallNamedViewDto =
        serde_json::from_value(applied["result"].clone()).unwrap();
    let mut rejected = applied.clone();
    rejected["applied"] = json!(false);
    after_mutation(app.world_mut(), "inbox", &rejected).unwrap();
    let (_, unchanged_camera, unchanged_presentation, _) =
        native_viewport::interface_view_snapshot(app.world());
    assert_eq!(unchanged_camera.position, camera.position);
    assert_eq!(unchanged_camera.target, camera.target);
    assert_eq!(unchanged_camera.up, camera.up);
    assert!(unchanged_presentation.selected_surface_point.is_some());
    let mutation = NativeMutationResult {
        context: owner,
        engine_revision: applied["engine_revision"].as_u64().unwrap(),
        value: applied.clone(),
    };
    let prepared =
        prepare_native_presentation(&fixture.engine, &fixture.bridge, &mutation, "inbox");
    app.world_mut().insert_resource(prepared);
    let outcome = finish_mutation(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        "inbox",
        mutation,
    );
    assert!(
        outcome["render_error"].is_null(),
        "Native named-view presentation returned an error"
    );
    assert_eq!(outcome["operation"], "inbox");
    assert!(
        outcome["result"] == applied,
        "Keep the existing inbox receipt shape"
    );
    let (_, camera, presentation, _) = native_viewport::interface_view_snapshot(app.world());
    assert_eq!(camera.position, saved.camera.position.map(|v| v as f32));
    assert_eq!(camera.target, saved.camera.target.map(|v| v as f32));
    assert_eq!(camera.up, saved.camera.up.map(|v| v as f32));
    assert_eq!(
        presentation.hidden_body_ids,
        recalled.visibility.hidden_body_ids
    );
    assert_eq!(*presentation.body_poses, recalled.solution.body_poses);
    assert_eq!(
        *presentation.instance_body_poses,
        recalled.solution.instance_body_poses
    );
    assert!(presentation.selected_surface_point.is_none());
    assert!(
        serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap() == source,
        "Named-view recall must preserve exact source geometry"
    );
}

#[test]
fn translated_and_rotated_saved_view_resets_before_source_forms_without_losing_ids() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let saved = solid_and_view(&fixture);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "recall_named_view",
            &json!({"name":saved.name}),
            || Ok(()),
        )
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let (document, _, mut presentation, _) = native_viewport::interface_view_snapshot(app.world());
    presentation.selected_body_ids = saved.visible_body_ids.clone();
    presentation.selected_occurrence_id = Some(saved.occurrence_offsets[0].occurrence_id.0);
    presentation.selected_face_ids = vec![17];
    presentation.selected_surface_point = Some(limo_cad_solid::Point3Dto {
        x: 100.,
        y: 20.,
        z: 30.,
    });
    native_viewport::apply_interface_view(app.world_mut(), &document, None, Some(presentation))
        .unwrap();
    ensure_source_ready(
        app.world_mut(),
        &fixture.engine,
        &fixture.bridge,
        &fixture.owner(),
    )
    .unwrap();
    assert!(read_views(&fixture.engine).unwrap().active.is_none());
    let presentation = native_viewport::interface_view_snapshot(app.world()).2;
    assert!(presentation.selected_surface_point.is_none());
    assert_eq!(presentation.selected_body_ids, saved.visible_body_ids);
    assert_eq!(
        presentation.selected_occurrence_id,
        Some(saved.occurrence_offsets[0].occurrence_id.0)
    );
    assert_eq!(presentation.selected_face_ids, vec![17]);
    let current: AssemblySolutionDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("assembly_solution", "")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        *presentation.instance_body_poses,
        current.instance_body_poses
    );
}

#[test]
fn draft_preview_rejects_hole_open_and_preserves_draft_while_assembled_picks_survive() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let saved = solid_and_view(&fixture);
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let (document, _, mut presentation, _) = native_viewport::interface_view_snapshot(app.world());
    presentation.selected_surface_point = Some(limo_cad_solid::Point3Dto {
        x: 3.,
        y: 2.,
        z: 3.,
    });
    native_viewport::apply_interface_view(app.world_mut(), &document, None, Some(presentation))
        .unwrap();
    ensure_source_ready(
        app.world_mut(),
        &fixture.engine,
        &fixture.bridge,
        &fixture.owner(),
    )
    .unwrap();
    assert!(native_viewport::interface_view_snapshot(app.world())
        .2
        .selected_surface_point
        .is_some());
    app.world_mut().insert_resource(State {
        draft: Some(saved.clone()),
        previewing: true,
        ..default()
    });
    let error = feature::reduce(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &fixture.owner(),
        &feature::FeatureCommand::Open {
            kind: feature::SolidFormKind::Hole,
            feature_id: None,
        },
        &ControlInput::Click,
        || Ok(()),
    )
    .unwrap_err();
    assert!(error.contains("Reset to assembled placement"), "{error}");
    assert!(feature::panel(app.world()).is_none());
    assert_eq!(app.world().resource::<State>().draft.as_ref(), Some(&saved));
    assert!(app.world().resource::<State>().previewing);
}

#[test]
fn corrections_translate_existing_groups_without_replacing_their_rotation() {
    let mut draft = view();
    let rotation = draft.occurrence_offsets[0].rotation;
    apply_corrections(
        &mut draft,
        &json!({"proposal_fits":true,"proposed_translations":[
            {"occurrence_id":42,"translation":[-4.,5.,-30.]},
            {"occurrence_id":99,"translation":[200.,0.,0.]}
        ]}),
    )
    .unwrap();
    assert_eq!(draft.occurrence_offsets[0].translation, [6., 25., 0.]);
    assert_eq!(draft.occurrence_offsets[0].rotation, rotation);
    assert_eq!(draft.occurrence_offsets[1].rotation, [0., 0., 0., 1.]);
    let before = draft.clone();
    assert!(apply_corrections(&mut draft, &json!({"proposal_fits":false})).is_err());
    assert_eq!(draft, before);
}

#[test]
fn occurrence_measurements_use_existing_cad_units_and_leave_other_instances_untouched() {
    let mut state = State::default();
    select(&mut state, view(), true);
    state.target = "occurrence:42".into();
    state.placement = Some(assembly::TransformDraft::new(
        target_pose(state.draft.as_ref().unwrap(), &state.target),
        UnitSystem::In,
    ));
    state.placement.as_mut().unwrap().translation[0].set_text("2 in".into());
    state.placement_dirty = true;
    apply_placement(&mut state, UnitSystem::In).unwrap();
    let draft = state.draft.as_ref().unwrap();
    assert_eq!(draft.occurrence_offsets.len(), 1);
    assert_eq!(draft.occurrence_offsets[0].occurrence_id.0, 42);
    assert_eq!(draft.occurrence_offsets[0].translation, [50.8, 20., 30.]);
    assert_eq!(
        draft.occurrence_offsets[0].rotation,
        view().occurrence_offsets[0].rotation
    );
    let before = draft.clone();
    state.placement.as_mut().unwrap().translation[0].set_text("NaN".into());
    state.placement_dirty = true;
    assert!(apply_placement(&mut state, UnitSystem::In).is_err());
    assert_eq!(state.draft.as_ref(), Some(&before));
}

#[test]
fn reset_removes_stale_world_coordinates_but_retains_occurrence_and_face_identity() {
    let mut presentation = native_viewport::ViewportPresentation {
        selected_body_ids: vec![7],
        selected_occurrence_id: Some(42),
        selected_face_ids: vec![17],
        selected_surface_point: Some(limo_cad_solid::Point3Dto {
            x: 100.,
            y: 0.,
            z: 0.,
        }),
        ..Default::default()
    };
    clear_pick_coordinates(&mut presentation);
    assert!(presentation.selected_surface_point.is_none());
    assert_eq!(presentation.selected_body_ids, vec![7]);
    assert_eq!(presentation.selected_occurrence_id, Some(42));
    assert_eq!(presentation.selected_face_ids, vec![17]);
}

#[test]
fn draft_preview_requires_an_explicit_save_recall_or_reset_before_export() {
    let mut world = World::new();
    world.insert_resource(State {
        previewing: true,
        ..default()
    });
    assert!(ensure_exportable(&world).is_err());
    world.resource_mut::<State>().previewing = false;
    assert!(ensure_exportable(&world).is_ok());
}

#[test]
fn capturing_an_active_view_copies_offsets_and_current_camera_without_mutating_the_document() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let saved = solid_and_view(&fixture);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "recall_named_view",
            &json!({"name":saved.name}),
            || Ok(()),
        )
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let original =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let current = native_viewport::ViewportCamera {
        position: [10., 20., 30.],
        target: [1., 2., 3.],
        ..default()
    };
    native_viewport::apply_interface_view(
        app.world_mut(),
        &fixture.owner().document_id,
        Some(current),
        None,
    )
    .unwrap();
    let captured = capture(app.world(), &fixture.engine, "New view".into()).unwrap();
    assert_eq!(captured.camera.position, [10., 20., 30.]);
    assert_eq!(captured.camera.target, [1., 2., 3.]);
    assert_eq!(captured.occurrence_offsets, saved.occurrence_offsets);
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        original
    );
}

#[test]
fn capture_rejects_a_viewport_from_a_previous_document_without_creating_a_view() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid_and_view(&fixture);
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    parse_engine_envelope(fixture.bridge.with_project_session_transition(
        "main",
        &fixture.engine,
        || fixture.engine.create_project_session("new-document"),
    ))
    .unwrap();
    let before = read_views(&fixture.engine).unwrap();
    assert!(
        capture(app.world(), &fixture.engine, "Stale capture".into())
            .unwrap_err()
            .contains("changing documents")
    );
    assert_eq!(read_views(&fixture.engine).unwrap(), before);
}

#[test]
fn named_views_cannot_take_over_picking_from_an_open_source_hole_form() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid_and_view(&fixture);
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    feature::reduce(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &fixture.owner(),
        &feature::FeatureCommand::Open {
            kind: feature::SolidFormKind::Hole,
            feature_id: None,
        },
        &ControlInput::Click,
        || Ok(()),
    )
    .unwrap();
    let original = feature::panel(app.world()).unwrap().form_id;
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    assert!(
        open(app.world_mut(), &fixture.engine, &fixture.owner(), None)
            .unwrap_err()
            .contains("cancel the source feature")
    );
    assert_eq!(feature::panel(app.world()).unwrap().form_id, original);
    assert!(!active(app.world()));
    assert!(activate(
        app.world_mut(),
        &fixture.engine,
        &fixture.bridge,
        &fixture.owner(),
        "Fixture view"
    )
    .is_err());
    assert_eq!(feature::panel(app.world()).unwrap().form_id, original);
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &fixture.owner(), "solid")
        .unwrap();
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let revision = fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    let owner = fixture.owner();
    let root = crate::session_bridge::session_root()
        .join(&session)
        .join("inbox");
    std::fs::create_dir_all(&root).unwrap();
    {
        let (seq, name, arguments) = (1, "recall_named_view", json!({"name":"Fixture view"}));
        std::fs::write(root.join(format!("{seq}.json")), json!({"name":name,"arguments":arguments,
            "base_generation":revision,"session_id":session,"window_id":"main","document_id":owner.document_id}).to_string()).unwrap();
        let result = crate::session_bridge::apply_or_reject_one_inbox_op_with_presentation_guard(
            &fixture.bridge,
            "main",
            &fixture.engine,
            None,
            Some((&owner.document_id, &session)),
            presentation_locked(app.world()),
        )
        .unwrap();
        assert_eq!(result["dead_lettered"], true);
        assert_eq!(result["reason"], "presentation_editor_active");
        assert_eq!(
            fixture.bridge.engine_revision_for_window("main").unwrap(),
            Some(revision)
        );
    }
    assert!(limo_cad_mcp_mutate::is_live_engine_query("named_views"));
    assert_eq!(read_views(&fixture.engine).unwrap().views.len(), 1);
    assert!(read_views(&fixture.engine).unwrap().active.is_none());
    assert_eq!(feature::panel(app.world()).unwrap().form_id, original);
}
