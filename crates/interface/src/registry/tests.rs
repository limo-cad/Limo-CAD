use super::*;

fn context() -> DocumentContext {
    DocumentContext {
        window_id: "main".into(),
        document_id: "part-A".into(),
        epoch: 1,
    }
}
fn button(key: u64, label: &str) -> Control {
    Control {
        key: ControlKey(key),
        binding: 1,
        surface: "solid/build".into(),
        label: label.into(),
        role: "button".into(),
        bounds: Rect {
            x: 4.,
            y: 8.,
            width: 80.,
            height: 32.,
        },
        visible: true,
        disabled: false,
        expanded: None,
        selected: None,
        field: Field::None,
        modal_scope: None,
        text_editing: false,
        owned_keys: Vec::new(),
    }
}
fn frame(controls: Vec<Control>) -> SurfaceFrame {
    SurfaceFrame {
        controls,
        client: Rect {
            width: 1200.,
            height: 800.,
            ..Rect::default()
        },
        document_visible: true,
        ..SurfaceFrame::default()
    }
}
fn registry(controls: Vec<Control>) -> SurfaceRegistry {
    let mut registry = SurfaceRegistry::new();
    registry.replace(context(), frame(controls)).unwrap();
    registry
}

#[test]
fn ranges_share_finite_bounds_keyboard_steps_and_live_validation() {
    let mut slider = button(1, "Motion time");
    slider.role = "slider".into();
    slider.field = Field::Range {
        value: 2.,
        min: 0.,
        max: 5.,
        step: 0.1,
    };
    let mut r = registry(vec![slider.clone()]);
    let context = context();
    let inspected = r.inspect().unwrap();
    let c = &inspected["surfaces"][0]["controls"][0];
    assert_eq!(c["value"], 2.);
    assert_eq!(c["min"], 0.);
    assert_eq!(c["max"], 5.);
    assert_eq!(c["step"], 0.1);
    for value in ["NaN", "inf", "-0.1", "5.1", "nonsense"] {
        assert_eq!(
            r.resolve_key(
                ControlKey(1),
                ControlInput::SetValue(value.into()),
                &context
            )
            .unwrap_err(),
            ControlError::InvalidValue
        );
    }
    for (key, value) in [
        ("Home", 0.),
        ("End", 5.),
        ("ArrowRight", 2.1),
        ("ArrowLeft", 1.9),
    ] {
        let action = r
            .resolve_key(
                ControlKey(1),
                ControlInput::Key(KeyChord::plain(key)),
                &context,
            )
            .unwrap();
        let ControlInput::SetValue(actual) = action.input else {
            panic!("Range key was not normalized");
        };
        assert!((actual.parse::<f64>().unwrap() - value).abs() < 1e-12);
    }
    let action = r
        .resolve_key(ControlKey(1), ControlInput::SetValue("4".into()), &context)
        .unwrap();
    slider.field = Field::Range {
        value: 1.,
        min: 0.,
        max: 3.,
        step: 0.1,
    };
    r.replace(context.clone(), frame(vec![slider.clone()]))
        .unwrap();
    assert_eq!(
        r.validate_resolved(&action, &context),
        Err(ControlError::InvalidValue)
    );
    for bad in [
        Field::Range {
            value: 0.,
            min: 0.,
            max: 0.,
            step: 1.,
        },
        Field::Range {
            value: 0.,
            min: 0.,
            max: 1.,
            step: 0.,
        },
        Field::Range {
            value: f64::NAN,
            min: 0.,
            max: 1.,
            step: 0.1,
        },
    ] {
        slider.field = bad;
        assert!(r
            .replace(context.clone(), frame(vec![slider.clone()]))
            .is_err());
    }
}
fn target(snapshot: &Value, label: &str) -> String {
    snapshot["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|surface| surface["controls"].as_array().unwrap())
        .find(|control| control["label"] == label)
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn inspection_expires_targets_without_retiring_retained_controls() {
    let mut registry = registry(vec![button(21, "Extrude")]);
    let first = target(&registry.inspect().unwrap(), "Extrude");
    let request = ControlRequest::Click {
        target: first.clone(),
    };
    assert_eq!(
        registry.resolve(&request, &context()).unwrap(),
        registry
            .resolve_key(ControlKey(21), ControlInput::Click, &context())
            .unwrap()
    );
    let second = target(&registry.inspect().unwrap(), "Extrude");
    assert_ne!(first, second);
    assert_eq!(
        registry.resolve(&request, &context()),
        Err(ControlError::Stale)
    );
    let mut other = super::tests::registry(vec![button(21, "Extrude")]);
    let other_id = target(&other.inspect().unwrap(), "Extrude");
    assert_ne!(
        second, other_id,
        "Two windows must never emit interchangeable target IDs"
    );
}

#[test]
fn stale_document_epoch_and_recycled_or_relabelled_instances_are_rejected() {
    let mut registry = registry(vec![button(21, "Extrude")]);
    let request = ControlRequest::Click {
        target: target(&registry.inspect().unwrap(), "Extrude"),
    };
    registry
        .replace(context(), frame(vec![button(22, "Extrude")]))
        .unwrap();
    assert_eq!(
        registry.resolve(&request, &context()),
        Err(ControlError::Stale)
    );
    registry
        .replace(context(), frame(vec![button(21, "Revolve")]))
        .unwrap();
    assert_eq!(
        registry.resolve(&request, &context()),
        Err(ControlError::Changed)
    );
    let mut replacement = context();
    replacement.epoch += 1;
    registry
        .replace(replacement.clone(), frame(vec![button(21, "Extrude")]))
        .unwrap();
    assert_eq!(
        registry.resolve_key(ControlKey(21), ControlInput::Click, &context()),
        Err(ControlError::DocumentChanged)
    );
    let fresh = ControlRequest::Click {
        target: target(&registry.inspect().unwrap(), "Extrude"),
    };
    assert_eq!(
        registry.resolve(&request, &replacement),
        Err(ControlError::Stale)
    );
    assert!(registry.resolve(&fresh, &replacement).is_ok());
    let mut foreign = replacement.clone();
    foreign.window_id = "other".into();
    assert_eq!(
        registry.resolve(&fresh, &foreign),
        Err(ControlError::DocumentChanged)
    );
}

#[test]
fn latest_visibility_disabled_and_top_modal_checks_apply_to_both_input_routes() {
    let mut registry = registry(vec![button(21, "Extrude")]);
    let request = ControlRequest::Click {
        target: target(&registry.inspect().unwrap(), "Extrude"),
    };
    let mut control = button(21, "Extrude");
    control.disabled = true;
    registry
        .replace(context(), frame(vec![control.clone()]))
        .unwrap();
    assert_eq!(
        registry.resolve(&request, &context()),
        Err(ControlError::Disabled)
    );
    assert_eq!(
        registry.resolve_key(control.key, ControlInput::Click, &context()),
        Err(ControlError::Disabled)
    );
    assert_eq!(
        registry.inspect().unwrap()["surfaces"][0]["controls"][0]["disabled"],
        true
    );
    control.disabled = false;
    control.visible = false;
    registry.replace(context(), frame(vec![control])).unwrap();
    assert_eq!(
        registry.resolve_key(ControlKey(21), ControlInput::Click, &context()),
        Err(ControlError::Stale)
    );
    assert_eq!(registry.inspect().unwrap()["surfaces"], json!([]));

    let mut outer = button(22, "Settings");
    outer.modal_scope = Some("dialogs/settings".into());
    let mut inner = button(23, "Cancel");
    inner.modal_scope = Some("dialogs/save".into());
    let mut portal = button(24, "Save choice");
    portal.surface = "menus/save-options".into();
    portal.modal_scope = inner.modal_scope.clone();
    let mut modal_frame = frame(vec![button(21, "Extrude"), outer, inner, portal]);
    modal_frame.surfaces = vec![
        Surface {
            name: "dialogs/settings".into(),
            text: None,
        },
        Surface {
            name: "dialogs/save".into(),
            text: Some("Save your part?".into()),
        },
    ];
    modal_frame.modal_stack = vec!["dialogs/settings".into(), "dialogs/save".into()];
    modal_frame.focused = Some(ControlKey(23));
    registry.replace(context(), modal_frame).unwrap();
    for key in [21, 22] {
        assert_eq!(
            registry.resolve_key(ControlKey(key), ControlInput::Click, &context()),
            Err(ControlError::ModalBlocked)
        );
    }
    for key in [23, 24] {
        assert!(registry
            .resolve_key(ControlKey(key), ControlInput::Click, &context())
            .is_ok());
    }
    assert_eq!(registry.focus_target(false), Some(ControlKey(24)));
    assert_eq!(registry.focus_target(true), Some(ControlKey(24)));
    assert_eq!(
        registry.keyboard_route(&context(), &KeyChord::plain("Escape")),
        Ok(KeyboardRoute::Modal)
    );
}

#[test]
fn edit_validation_does_not_replace_the_real_handler_or_its_focus_and_commit_events() {
    let mut text = button(1, "Distance");
    text.role = "number".into();
    text.field = Field::Text {
        value: "12".into(),
        read_only: false,
        selection: None,
    };
    let mut choice = button(2, "Extent");
    choice.role = "select".into();
    choice.field = Field::Choice {
        value: "distance".into(),
        options: vec![
            ChoiceOption {
                value: "distance".into(),
                label: "Distance".into(),
                disabled: false,
            },
            ChoiceOption {
                value: "through".into(),
                label: "Through all".into(),
                disabled: true,
            },
        ],
    };
    let mut toggle = button(3, "Flip");
    toggle.field = Field::Toggle(false);
    let mut registry = registry(vec![text, choice, toggle]);
    let snapshot = registry.inspect().unwrap();
    let set = |label: &str, value: &str| ControlRequest::SetValue {
        target: target(&snapshot, label),
        value: value.into(),
    };
    assert_eq!(
        registry
            .resolve(&set("Distance", "2 * 8"), &context())
            .unwrap()
            .input,
        ControlInput::SetValue("2 * 8".into())
    );
    assert!(
        matches!(&registry.frame.controls[0].field, Field::Text {value,..} if value == "12"),
        "Validation cannot silently mutate the model/form"
    );
    assert_eq!(
        registry.resolve(&set("Extent", "through"), &context()),
        Err(ControlError::OptionUnavailable)
    );
    assert_eq!(
        registry.resolve(&set("Flip", "true"), &context()),
        Err(ControlError::NotEditable)
    );
    registry.frame.controls[0].field = Field::Text {
        value: "12".into(),
        read_only: true,
        selection: None,
    };
    assert_eq!(
        registry.resolve(&set("Distance", "14"), &context()),
        Err(ControlError::ReadOnly)
    );
    let snapshot = registry.inspect().unwrap();
    let distance = snapshot["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|s| s["controls"].as_array().unwrap())
        .find(|c| c["label"] == "Distance")
        .unwrap();
    assert_eq!(distance["read_only"], true);
}

#[test]
fn keyboard_routing_preserves_field_editing_and_toggle_application_shortcuts() {
    let mut text = button(1, "Recipe");
    text.field = Field::Text {
        value: String::new(),
        read_only: false,
        selection: None,
    };
    let mut toggle = button(2, "Grid");
    toggle.field = Field::Toggle(true);
    let mut registry = registry(vec![text, toggle]);
    registry.frame.focused = Some(ControlKey(1));
    for key in ["s", "o", "n", "Escape", "Delete"] {
        assert_eq!(
            registry.keyboard_route(&context(), &KeyChord::plain(key)),
            Ok(KeyboardRoute::Widget)
        );
    }
    registry.frame.focused = Some(ControlKey(2));
    for key in ["s", "o", "n"] {
        assert_eq!(
            registry.keyboard_route(&context(), &KeyChord::plain(key)),
            Ok(KeyboardRoute::Model)
        );
    }
    assert_eq!(
        registry.keyboard_route(&context(), &KeyChord::plain(" ")),
        Ok(KeyboardRoute::Widget)
    );
    assert_eq!(
        registry.keyboard_route(&context(), &KeyChord::plain("Tab")),
        Ok(KeyboardRoute::Widget)
    );
    for key in [
        "Enter",
        "Escape",
        "ArrowUp",
        "ArrowDown",
        "ArrowLeft",
        "ArrowRight",
        "Home",
        "Delete",
        "Backspace",
    ] {
        let request: ControlRequest =
            serde_json::from_value(json!({"action":"key","target":"fresh","key":key})).unwrap();
        assert!(matches!(request, ControlRequest::Key { .. }));
    }
    assert!(serde_json::from_value::<ControlRequest>(
        json!({"action":"key","target":"fresh","key":"arbitrary script"})
    )
    .is_err());
}

#[test]
fn long_unicode_text_is_bounded_around_the_caret_without_modifying_the_field() {
    let value = "🦀".repeat(5000) + "final";
    let length = value.encode_utf16().count();
    let mut field = button(1, "Script");
    field.role = "textarea".into();
    field.field = Field::Text {
        value: value.clone(),
        read_only: false,
        selection: Some(TextSelection {
            start: length,
            end: length,
        }),
    };
    let mut registry = registry(vec![field]);
    let inspected = registry.inspect().unwrap();
    let text = &inspected["surfaces"][0]["controls"][0];
    assert_eq!(text["value_length"], length);
    assert_eq!(text["value_truncated"], true);
    let start = text["value_start"].as_u64().unwrap() as usize;
    let excerpt = text["value"].as_str().unwrap();
    assert!(excerpt.ends_with("final"));
    assert!(excerpt.encode_utf16().count() <= 4096);
    assert_eq!(
        value.encode_utf16().skip(start).collect::<Vec<_>>(),
        excerpt.encode_utf16().collect::<Vec<_>>()
    );
    assert!(
        matches!(&registry.frame.controls[0].field,Field::Text{value:actual,..} if actual == &value)
    );
}

#[test]
fn invalid_frames_fail_atomically_and_do_not_invalidate_a_good_snapshot() {
    let mut registry = registry(vec![button(1, "Extrude")]);
    let request = ControlRequest::Click {
        target: target(&registry.inspect().unwrap(), "Extrude"),
    };
    let mut bad = frame(vec![button(1, "Extrude"), button(1, "Revolve")]);
    assert!(matches!(
        registry.replace(context(), bad.clone()),
        Err(ControlError::InvalidFrame(_))
    ));
    bad.controls.pop();
    bad.controls[0].bounds.width = f64::NAN;
    assert!(registry.replace(context(), bad).is_err());
    assert!(registry.resolve(&request, &context()).is_ok());
}

#[test]
fn canonical_catalog_has_unique_groups_and_one_group_per_operation() {
    let mut groups = HashSet::new();
    let mut operations = HashSet::new();
    for group in catalog::groups() {
        let id = group["id"].as_str().unwrap();
        assert!(groups.insert(id), "Duplicate public interface group: {id}");
        for operation in group["operations"].as_array().unwrap() {
            let operation = operation.as_str().unwrap();
            assert!(
                operations.insert(operation),
                "Duplicate public operation: {operation}"
            );
            assert_eq!(catalog::group_for(operation), Some(id));
        }
    }
    for workspace in catalog::document()["workspaces"].as_array().unwrap() {
        for panel in workspace["panels"].as_array().unwrap() {
            assert!(groups.contains(
                format!(
                    "{}/{}",
                    workspace["id"].as_str().unwrap(),
                    panel["id"].as_str().unwrap()
                )
                .as_str()
            ));
        }
    }
}

#[test]
fn queued_actions_cannot_inherit_a_retained_widgets_replacement_binding() {
    let mut registry = registry(vec![button(1, "Edit")]);
    let target = target(&registry.inspect().unwrap(), "Edit");
    let request = ControlRequest::Click { target };
    let queued = registry.resolve(&request, &context()).unwrap();
    let human = registry
        .resolve_key(ControlKey(1), ControlInput::Click, &context())
        .unwrap();
    assert_eq!(queued, human);
    assert!(registry.validate_resolved(&queued, &context()).is_ok());
    let mut replacement = button(1, "Edit");
    replacement.binding = 2;
    registry
        .replace(context(), frame(vec![replacement]))
        .unwrap();
    assert_eq!(
        registry.resolve(&request, &context()),
        Err(ControlError::Changed)
    );
    for queued in [queued, human] {
        assert_eq!(
            registry.validate_resolved(&queued, &context()),
            Err(ControlError::Changed)
        );
    }
    let latest = registry
        .resolve_key(ControlKey(1), ControlInput::Click, &context())
        .unwrap();
    let mut disabled = registry.frame().clone();
    disabled.controls[0].disabled = true;
    registry.replace(context(), disabled).unwrap();
    assert_eq!(
        registry.validate_resolved(&latest, &context()),
        Err(ControlError::Disabled)
    );
    let mut owner = context();
    owner.epoch += 1;
    registry
        .replace(owner, frame(vec![button(1, "Edit")]))
        .unwrap();
    assert_eq!(
        registry.validate_resolved(&latest, &context()),
        Err(ControlError::DocumentChanged)
    );
}

#[test]
fn widgets_declare_directional_and_modified_keys_without_stealing_model_navigation() {
    let mut tab = button(1, "Program");
    tab.role = "tab".into();
    tab.owned_keys = ["ArrowLeft", "ArrowRight", "Home", "End"]
        .map(KeyChord::plain)
        .to_vec();
    let mut slider = button(2, "Rollback");
    slider.role = "slider".into();
    slider.owned_keys = ["ArrowLeft", "ArrowRight", "Home", "End"]
        .map(KeyChord::plain)
        .to_vec();
    let menu_key = KeyChord {
        shift: true,
        ..KeyChord::plain("F10")
    };
    let mut tree = button(3, "Body");
    tree.role = "treeitem".into();
    tree.owned_keys = vec![KeyChord::plain("ContextMenu"), menu_key.clone()];
    let mut registry = registry(vec![tab, slider, tree, button(4, "Fit")]);
    for key in [1, 2] {
        registry.frame.focused = Some(ControlKey(key));
        for chord in ["ArrowRight", "End"] {
            assert_eq!(
                registry.keyboard_route(&context(), &KeyChord::plain(chord)),
                Ok(KeyboardRoute::Widget)
            );
        }
    }
    registry.frame.focused = Some(ControlKey(3));
    assert_eq!(
        registry.keyboard_route(&context(), &menu_key),
        Ok(KeyboardRoute::Widget)
    );
    let queued = registry
        .resolve_key(
            ControlKey(3),
            ControlInput::Key(menu_key.clone()),
            &context(),
        )
        .unwrap();
    assert_eq!(queued.input, ControlInput::Key(menu_key));
    assert_eq!(
        registry.keyboard_route(&context(), &KeyChord::plain("F10")),
        Ok(KeyboardRoute::Model)
    );
    registry.frame.focused = Some(ControlKey(4));
    assert_eq!(
        registry.keyboard_route(&context(), &KeyChord::plain("ArrowRight")),
        Ok(KeyboardRoute::Model)
    );
    let save = KeyChord {
        ctrl: true,
        ..KeyChord::plain("s")
    };
    assert_eq!(
        registry.keyboard_route(&context(), &save),
        Ok(KeyboardRoute::Model)
    );
}
