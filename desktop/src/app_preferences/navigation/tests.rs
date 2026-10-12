use super::*;

#[test]
fn navigation_defaults_preserve_every_existing_modifier_chord() {
    let bindings = Bindings::default();
    assert!(bindings.conflicts().is_empty());
    for bits in 0..32 {
        let shifted = bits & SHIFT != 0;
        let controlled = bits & CTRL != 0;
        assert_eq!(
            bindings.drag_action(Button::Middle, bits),
            Some(if shifted { Action::Orbit } else { Action::Pan })
        );
        assert_eq!(
            bindings.drag_action(Button::Right, bits),
            Some(Action::Orbit)
        );
        assert_eq!(bindings.drag_action(Button::Left, bits), None);
        for pixel in [false, true] {
            let expected = if shifted {
                Action::Orbit
            } else if controlled || !pixel {
                Action::Zoom
            } else {
                Action::Pan
            };
            assert_eq!(
                bindings.scroll_action(pixel, bits),
                expected,
                "pixel={pixel}, modifiers={bits}"
            );
        }
    }
}

#[test]
fn navigation_conflicts_detect_broad_chords_and_leave_disjoint_contexts_available() {
    let mut bindings = Bindings::default();
    bindings.zoom = DragBinding {
        button: Button::Right,
        modifiers: Modifiers::Ctrl,
    };
    let conflicts = bindings.conflicts();
    assert!(conflicts.contains(&Conflict::Overlap(
        DragSlot::OrbitAlternative,
        DragSlot::Zoom,
        Button::Right,
        CTRL
    )));
    assert!(bindings.validate().unwrap_err().contains("Ctrl+Right"));
    bindings.orbit_alternative.button = Button::Disabled;
    bindings.zoom.button = Button::Middle;
    assert!(bindings.conflicts().iter().any(|conflict| matches!(
        conflict,
        Conflict::Overlap(DragSlot::Pan, DragSlot::Zoom, _, _)
    )));
    bindings.pan.modifiers = Modifiers::None;
    assert!(bindings.validate().is_ok());
    assert_eq!(
        bindings.drag_action(Button::Middle, CTRL),
        Some(Action::Zoom)
    );
    assert_eq!(
        bindings.drag_action(Button::Middle, CTRL | SHIFT),
        Some(Action::Orbit)
    );
    assert_eq!(
        bindings.drag_action(Button::Middle, CTRL | ALT),
        None,
        "An exact Ctrl chord must not swallow a different chord"
    );
    bindings.zoom.button = Button::Left;
    assert!(bindings
        .conflicts()
        .contains(&Conflict::Selection(DragSlot::Zoom)));
    bindings.zoom.button = Button::Right;
    // Scroll sources have distinct contexts and can deliberately share actions.
    bindings.wheel = Action::Pan;
    bindings.trackpad = Action::Pan;
    bindings.shift_scroll = Action::Inherit;
    bindings.ctrl_scroll = Action::Inherit;
    bindings.alt_scroll = Action::Zoom;
    assert!(bindings.validate().is_ok());
    assert_eq!(
        bindings.scroll_action(true, SHIFT | CTRL | ALT),
        Action::Zoom
    );
    assert_eq!(bindings.scroll_action(false, 0), Action::Pan);
    bindings.wheel = Action::Inherit;
    assert!(bindings
        .conflicts()
        .contains(&Conflict::BaseScroll(ScrollSlot::Wheel)));
}
