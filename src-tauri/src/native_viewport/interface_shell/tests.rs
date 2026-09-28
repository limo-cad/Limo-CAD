use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

pub(crate) fn publish_layout_once(world: &mut World, handle: NativeInterfaceHandle) {
    world.insert_resource(handle);
    world.run_system_cached(publish_layout).unwrap();
}

#[test]
fn interface_camera_cannot_render_world_grid_or_transient_geometry() {
    use bevy::camera::visibility::RenderLayers;
    let mut app = App::new();
    app.add_systems(Update, setup_camera);
    app.update();
    let layers = app
        .world_mut()
        .query_filtered::<&RenderLayers, With<InterfaceCamera>>()
        .single(app.world())
        .unwrap();
    assert!(!layers.intersects(&RenderLayers::default()));
    assert!(!layers.intersects(&RenderLayers::layer(1)));
}

#[test]
fn scene_only_changes_require_a_new_submission_without_rebinding_controls() {
    let (mut app, handle, _, wakes) = fixture();
    let before = handle
        .read_surface(|_, frame| frame.controls.clone())
        .unwrap();
    let revision = handle.render_receipt().unwrap().laid_out_revision;
    handle.submitted_revision(revision).unwrap();
    handle.invalidate_presentation();
    app.update();
    let next = handle.render_receipt().unwrap().laid_out_revision;
    assert!(next > revision);
    assert_eq!(
        handle
            .read_surface(|_, frame| frame.controls.clone())
            .unwrap(),
        before
    );
    assert!(!handle.wait_for_submission(next));
    let armed = wakes.load(Ordering::Relaxed);
    handle.submitted_revision(revision).unwrap();
    assert_eq!(wakes.load(Ordering::Relaxed), armed);
    handle.submitted_revision(next).unwrap();
    assert_eq!(wakes.load(Ordering::Relaxed), armed + 1);
    handle.submitted_revision(next).unwrap();
    assert_eq!(wakes.load(Ordering::Relaxed), armed + 1);
}

fn frame(document: &str, epoch: u64) -> InterfaceFrame {
    let client = InterfaceRect {
        x: 0.0,
        y: 0.0,
        width: 1000.0,
        height: 800.0,
    };
    let surface = InterfaceRect {
        x: 100.0,
        y: 100.0,
        width: 600.0,
        height: 500.0,
    };
    InterfaceFrame {
        context: DocumentContext {
            window_id: "main".into(),
            document_id: document.into(),
            epoch,
        },
        client,
        surface,
        canvases: vec![Canvas {
            name: "viewport".into(),
            bounds: surface,
        }],
        surfaces: vec![Surface {
            name: "Viewport".into(),
            text: None,
        }],
        modal_stack: vec![],
        document_visible: true,
    }
}

/// Headless layout-adapter fixture: real Bevy entity generations, with physical
/// output from layout supplied explicitly. GPU behavior has a separate native
/// render gate; these tests exercise ownership, clipping and input state.
pub(crate) fn fixture() -> (App, NativeInterfaceHandle, Entity, Arc<AtomicUsize>) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = wakes.clone();
    let handle = NativeInterfaceHandle::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    });
    let mut app = App::new();
    app.insert_resource(handle.clone())
        .add_systems(Update, publish_layout);
    let entity = app
        .world_mut()
        .spawn((
            InterfaceControl::button("Viewport", "Select"),
            ComputedNode {
                size: Vec2::new(80.0, 24.0),
                inverse_scale_factor: 1.0,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(60.0, 42.0)),
            ComputedStackIndex(1),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    handle.present(frame("document-a", 1)).unwrap();
    app.update();
    (app, handle, entity, wakes)
}

fn click(handle: &NativeInterfaceHandle) -> Result<(), String> {
    assert!(handle.pointer(PointerPhase::Down, [140.0, 140.0], PointerButton::Primary)?);
    assert!(handle.pointer(PointerPhase::Up, [140.0, 140.0], PointerButton::Primary)?);
    Ok(())
}

#[test]
fn range_drag_uses_real_bounds_coalesces_and_rejects_rebound_controls() {
    let (mut app, handle, entity, _) = fixture();
    let mut control = app.world_mut().get_mut::<InterfaceControl>(entity).unwrap();
    control.role = "slider".into();
    control.field = Field::Range {
        value: 0.,
        min: 0.,
        max: 10.,
        step: 1.,
    };
    drop(control);
    app.update();
    handle
        .pointer(PointerPhase::Down, [140., 140.], PointerButton::Primary)
        .unwrap();
    for x in 141..195 {
        handle
            .pointer(PointerPhase::Move, [x as f64, 140.], PointerButton::Primary)
            .unwrap();
    }
    handle
        .pointer(PointerPhase::Up, [600., 140.], PointerButton::Primary)
        .unwrap();
    let actions = handle.take_actions().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(
        actions[0].control.input,
        ControlInput::SetValue("10".into())
    );
    handle.validate_action(&actions[0]).unwrap();
    // A kernel preview can temporarily disable controls during a drag. The
    // release value stays queued and cannot apply until the control is enabled.
    handle.pointer(PointerPhase::Down,[140.,140.],PointerButton::Primary).unwrap();
    app.world_mut().get_mut::<InterfaceControl>(entity).unwrap().disabled=true;app.update();
    handle.pointer(PointerPhase::Move,[170.,140.],PointerButton::Primary).unwrap();
    handle.pointer(PointerPhase::Up,[600.,140.],PointerButton::Primary).unwrap();
    let actions=handle.take_actions().unwrap();assert_eq!(actions.len(),1);
    assert!(handle.validate_action(&actions[0]).is_err());
    app.world_mut().get_mut::<InterfaceControl>(entity).unwrap().disabled=false;app.update();
    handle.validate_action(&actions[0]).unwrap();
    handle
        .pointer(PointerPhase::Down, [140., 140.], PointerButton::Primary)
        .unwrap();
    handle.take_actions().unwrap();
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .binding += 1;
    app.update();
    assert!(handle
        .pointer(PointerPhase::Move, [160., 140.], PointerButton::Primary)
        .is_err());
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn painted_panel_blocks_geometry_and_underlying_controls_but_not_its_children() {
    let (mut app, handle, button, _) = fixture();
    let panel = app
        .world_mut()
        .spawn((
            InterfaceOccluder,
            ComputedNode {
                size: Vec2::new(160., 80.),
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(60., 42.)),
            ComputedStackIndex(2),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    app.update();
    assert!(handle.owns_pointer([140., 140.]));
    assert!(hit(&handle.shared.lock().unwrap(), [140., 140.]).is_none());
    click(&handle).unwrap();
    assert!(handle.take_actions().unwrap().is_empty());
    // A real field/button painted above the panel still receives its input.
    app.world_mut()
        .entity_mut(button)
        .insert(ComputedStackIndex(3));
    app.update();
    click(&handle).unwrap();
    assert_eq!(handle.take_actions().unwrap().len(), 1);
    // Removing the panel releases its otherwise blank area to the model.
    app.world_mut().despawn(panel);
    app.update();
    assert!(!handle.owns_pointer([230., 160.]));
}

fn request(handle: &NativeInterfaceHandle) -> ControlRequest {
    let inspected = handle.inspect().unwrap();
    ControlRequest::Click {
        target: inspected["surfaces"][0]["controls"][0]["id"]
            .as_str()
            .unwrap()
            .into(),
    }
}

#[test]
fn menu_keys_skip_disabled_entries_and_backdrops_for_human_and_mcp() {
    let (mut app, handle, _, _) = fixture();
    let mut menu_frame = frame("document-a", 1);
    menu_frame.surfaces.push(Surface {
        name: "menu".into(),
        text: None,
    });
    menu_frame.modal_stack.push("menu".into());
    let mut items = Vec::new();
    for index in 0..4 {
        let mut control = InterfaceControl::button("menu", format!("Item {index}"));
        control.modal_scope = Some("menu".into());
        control.role = if index == 3 { "button" } else { "menuitem" }.into();
        control.disabled = index == 1;
        control.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
            .into_iter()
            .map(KeyChord::plain)
            .collect();
        items.push(
            app.world_mut()
                .spawn((
                    control,
                    ComputedNode {
                        size: Vec2::new(80., 24.),
                        inverse_scale_factor: 1.,
                        ..default()
                    },
                    UiGlobalTransform::from_translation(Vec2::new(60., 82. + index as f32 * 30.)),
                    ComputedStackIndex(10 + index),
                    InheritedVisibility::VISIBLE,
                ))
                .id(),
        );
    }
    handle.present(menu_frame.clone()).unwrap();
    app.update();
    for (key, index) in [
        ("ArrowDown", 0),
        ("ArrowDown", 2),
        ("ArrowDown", 0),
        ("ArrowUp", 2),
        ("Home", 0),
        ("End", 2),
    ] {
        assert!(handle.key(KeyChord::plain(key)).unwrap());
        assert_eq!(
            handle.shared.lock().unwrap().focused,
            Some(ControlKey(items[index].to_bits()))
        );
        assert!(
            handle.take_actions().unwrap().is_empty(),
            "Navigation must not activate a command"
        );
    }
    app.update();
    let inspected = handle.inspect().unwrap();
    let item = inspected["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|surface| surface["controls"].as_array().unwrap())
        .find(|c| c["label"] == "Item 0")
        .unwrap();
    let action = handle
        .resolve(
            &ControlRequest::Key {
                target: item["id"].as_str().unwrap().into(),
                key: nbcad_interface::Key::ArrowDown,
            },
            &menu_frame.context,
        )
        .unwrap();
    handle.prepare_activation(&action).unwrap();
    let ControlInput::Key(key) = &action.control.input else {
        panic!("Expected key");
    };
    assert!(handle.navigate_menu(key).unwrap());
    assert_eq!(
        handle.shared.lock().unwrap().focused,
        Some(ControlKey(items[2].to_bits()))
    );
    for item in items {
        app.world_mut().despawn(item);
    }
    handle.present(frame("document-a", 1)).unwrap();
    app.update();
    assert!(!handle.navigate_menu(&KeyChord::plain("ArrowDown")).unwrap());
}

#[test]
fn native_pointer_and_mcp_resolve_the_same_retained_control() {
    let (mut app, handle, entity, _) = fixture();
    let from_mcp = handle
        .resolve(&request(&handle), &frame("document-a", 1).context)
        .unwrap();
    handle.prepare_activation(&from_mcp).unwrap();
    assert_eq!(
        handle.shared.lock().unwrap().focused,
        Some(ControlKey(entity.to_bits()))
    );
    assert!(handle.take_actions().unwrap().is_empty());
    click(&handle).unwrap();
    let from_pointer = handle.take_actions().unwrap().pop().unwrap();
    assert_eq!(from_pointer, from_mcp);
    assert_eq!(from_pointer.control.key, ControlKey(entity.to_bits()));
    handle.validate_action(&from_pointer).unwrap();
    let laid_out = handle.render_receipt().unwrap().laid_out_revision;
    app.update(); // Focus changed; this is one coherent semantic update.
    app.update();
    assert_eq!(handle.render_receipt().unwrap().laid_out_revision, laid_out);
}

#[test]
fn closing_a_modal_restores_previous_focus_only_if_the_control_survives() {
    let (mut app, handle, entity, _) = fixture();
    click(&handle).unwrap();
    handle.take_actions().unwrap();
    let mut modal = frame("document-a", 1);
    modal.modal_stack.push("settings".into());
    handle.present(modal.clone()).unwrap();
    app.update();
    assert_eq!(handle.shared.lock().unwrap().focused, None);
    handle.present(frame("document-a", 1)).unwrap();
    app.update();
    assert_eq!(
        handle.shared.lock().unwrap().focused,
        Some(ControlKey(entity.to_bits()))
    );
    handle.present(modal).unwrap();
    app.update();
    app.world_mut().despawn(entity);
    handle.present(frame("document-a", 1)).unwrap();
    app.update();
    assert_eq!(handle.shared.lock().unwrap().focused, None);
    assert!(handle.inspect().is_ok());
}

#[test]
fn submitted_receipt_cannot_claim_a_newer_unrendered_layout() {
    let (mut app, handle, entity, _) = fixture();
    let extracted = handle.render_receipt().unwrap().laid_out_revision;
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .label = "Changed".into();
    app.update();
    let current = handle.render_receipt().unwrap().laid_out_revision;
    assert!(current > extracted);
    handle.submitted_revision(extracted).unwrap();
    assert_eq!(
        handle.render_receipt().unwrap().submitted_revision,
        extracted
    );
    assert!(handle.submitted_revision(current + 1).is_err());
    handle.submitted_revision(current).unwrap();
    handle.submitted_revision(extracted).unwrap();
    assert_eq!(handle.render_receipt().unwrap().submitted_revision, current);
}

#[test]
fn drag_release_outside_and_capture_cancel_do_not_activate() {
    let (_, handle, _, _) = fixture();
    handle
        .pointer(PointerPhase::Down, [140.0, 140.0], PointerButton::Primary)
        .unwrap();
    assert!(handle
        .pointer(PointerPhase::Up, [900.0, 700.0], PointerButton::Primary)
        .unwrap());
    assert!(handle.take_actions().unwrap().is_empty());
    handle
        .pointer(PointerPhase::Down, [140.0, 140.0], PointerButton::Primary)
        .unwrap();
    handle
        .pointer(PointerPhase::Cancel, [140.0, 140.0], PointerButton::Primary)
        .unwrap();
    handle
        .pointer(PointerPhase::Up, [140.0, 140.0], PointerButton::Primary)
        .unwrap();
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn rebinding_between_press_and_release_cannot_activate_the_replacement() {
    let (mut app, handle, entity, _) = fixture();
    handle
        .pointer(PointerPhase::Down, [140.0, 140.0], PointerButton::Primary)
        .unwrap();
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .binding += 1;
    app.update();
    assert!(handle
        .pointer(PointerPhase::Up, [140.0, 140.0], PointerButton::Primary)
        .is_err());
    assert!(handle.take_actions().unwrap().is_empty());
    click(&handle).unwrap();
    let queued = handle.take_actions().unwrap().pop().unwrap();
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .disabled = true;
    app.update();
    assert!(handle.validate_action(&queued).is_err());
}

#[test]
fn pending_same_document_modal_fences_old_input_without_waiting_for_gpu() {
    let (mut app, handle, _, _) = fixture();
    let old = request(&handle);
    let mut modal = frame("document-a", 1);
    modal.surfaces.push(Surface {
        name: "dialog".into(),
        text: None,
    });
    modal.modal_stack.push("dialog".into());
    modal.document_visible = false;
    handle.present(modal.clone()).unwrap();
    assert!(handle.resolve(&old, &modal.context).is_err());
    assert!(handle
        .pointer(PointerPhase::Down, [900.0, 700.0], PointerButton::Primary)
        .unwrap());
    app.update();
    assert_eq!(handle.inspect().unwrap()["document_visible"], false);
    assert_eq!(handle.render_receipt().unwrap().submitted_revision, 0);
    assert!(handle.key(KeyChord::plain("Escape")).unwrap());
    let escape = handle.take_modal_keys().unwrap().pop().unwrap();
    handle.validate_modal_key(&escape).unwrap();
    handle.present(frame("document-a", 1)).unwrap();
    handle.present(modal).unwrap(); // A replacement modal may reuse its label.
    app.update();
    assert!(handle.validate_modal_key(&escape).is_err());
}

#[test]
fn replaced_document_revokes_queued_actions_and_native_hit_fallback() {
    let (mut app, handle, _, _) = fixture();
    let old = request(&handle);
    click(&handle).unwrap();
    handle.present(frame("document-b", 2)).unwrap();
    assert!(handle.take_actions().unwrap().is_empty());
    assert!(handle
        .pointer(PointerPhase::Down, [140.0, 140.0], PointerButton::Primary)
        .unwrap());
    assert!(handle
        .resolve(&old, &frame("document-a", 1).context)
        .is_err());
    app.update();
    assert!(handle.inspect().is_ok());
    click(&handle).unwrap();
    assert_eq!(
        handle.take_actions().unwrap()[0].context.document_id,
        "document-b"
    );
}

#[test]
fn removal_of_focused_controls_keeps_publication_live() {
    let (mut app, handle, entity, _) = fixture();
    click(&handle).unwrap();
    handle.take_actions().unwrap();
    app.world_mut().despawn(entity);
    app.update();
    let inspected = handle.inspect().unwrap();
    assert!(inspected["focused_control"].is_null());
    assert!(handle
        .shared
        .lock()
        .unwrap()
        .registry
        .frame()
        .controls
        .is_empty());
    assert!(!handle.key(KeyChord::plain("Enter")).unwrap());
}

#[test]
fn physical_coordinates_clipping_and_inherited_visibility_match_hit_bounds() {
    let (mut app, handle, entity, _) = fixture();
    let frame = handle.frame().unwrap();
    assert_eq!(
        frame.physical_to_window([80.0, 80.0], [1200.0, 1000.0]),
        Some([140.0, 140.0])
    );
    assert!(frame
        .physical_to_window([0.0, 0.0], [0.0, 1000.0])
        .is_none());
    app.world_mut().entity_mut(entity).insert(
        CalculatedClip::default().with_rect(
            Rect::from_corners(Vec2::new(50.0, 0.0), Vec2::new(100.0, 100.0)),
            &UiGlobalTransform::default(),
        ),
    );
    app.update();
    assert!(!handle
        .pointer(PointerPhase::Down, [140.0, 140.0], PointerButton::Primary)
        .unwrap());
    assert!(handle
        .pointer(PointerPhase::Down, [160.0, 140.0], PointerButton::Primary)
        .unwrap());
    handle.blur();
    app.world_mut()
        .entity_mut(entity)
        .insert(InheritedVisibility::HIDDEN);
    app.update();
    assert!(!handle
        .pointer(PointerPhase::Down, [160.0, 140.0], PointerButton::Primary)
        .unwrap());
}

#[test]
fn leaving_the_last_control_redraws_hover_without_consuming_model_input() {
    let (_, handle, _, wakes) = fixture();
    handle
        .pointer(PointerPhase::Move, [140.0, 140.0], PointerButton::Primary)
        .unwrap();
    let before = wakes.load(Ordering::Relaxed);
    assert!(!handle
        .pointer(PointerPhase::Move, [900.0, 700.0], PointerButton::Primary)
        .unwrap());
    assert_eq!(wakes.load(Ordering::Relaxed), before + 1);
}
