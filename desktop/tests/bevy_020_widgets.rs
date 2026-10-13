//! Characterization probes for the 0.20 experiment, not widget parity tests.
//! These record the integration work needed before replacing native routing.

use bevy::{
    input::{
        keyboard::{Key, KeyCode, KeyboardInput},
        ButtonInput, ButtonState,
    },
    input_focus::{FocusCause, FocusedInput, InputFocus},
    prelude::*,
    text::{EditableText, TextEdit},
    ui_widgets::{TextInput, TextInputPlugin},
    window::Ime,
};

fn fixture() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<ButtonInput<Key>>()
        .init_resource::<InputFocus>()
        .add_message::<Ime>()
        .add_plugins(TextInputPlugin);
    let field = app
        .world_mut()
        .spawn((TextInput, EditableText::new("12")))
        .id();
    app.world_mut()
        .get_mut::<EditableText>(field)
        .unwrap()
        .pending_edits
        .clear();
    let window = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(field, FocusCause::Navigated);
    (app, field, window)
}

fn type_text(app: &mut App, field: Entity, window: Entity, value: &str) {
    app.world_mut().trigger(FocusedInput::new(
        field,
        KeyboardInput {
            key_code: KeyCode::KeyQ,
            logical_key: Key::Character(value.into()),
            state: ButtonState::Pressed,
            text: Some(value.into()),
            repeat: false,
            window,
        },
        window,
    ));
}

#[test]
fn text_input_needs_an_explicit_flush_before_an_owned_submit() {
    let (mut app, field, window) = fixture();
    type_text(&mut app, field, window, "3");
    let editor = app.world().get::<EditableText>(field).unwrap();
    assert_eq!(editor.value().to_string(), "12");
    assert_eq!(editor.pending_edits, [TextEdit::Insert("3".into())]);
}

#[test]
fn text_input_does_not_insert_altgr_or_option_text_without_an_adapter() {
    let (mut app, field, window) = fixture();
    for modifiers in [vec![Key::Control, Key::Alt, Key::AltGraph], vec![Key::Alt]] {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<Key>>();
        keys.reset_all();
        for modifier in modifiers {
            keys.press(modifier);
        }

        type_text(&mut app, field, window, "@№");
        assert!(app
            .world()
            .get::<EditableText>(field)
            .unwrap()
            .pending_edits
            .is_empty());
    }
}

#[test]
fn ime_batch_targets_current_focus_and_does_not_capture_the_original_owner() {
    let (mut app, first, window) = fixture();
    app.world_mut().write_message(Ime::Commit {
        window,
        value: "日本".into(),
    });
    let second = app
        .world_mut()
        .spawn((TextInput, EditableText::new("other")))
        .id();
    app.world_mut()
        .get_mut::<EditableText>(second)
        .unwrap()
        .pending_edits
        .clear();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(second, FocusCause::Navigated);
    app.world_mut().run_schedule(PreUpdate);
    assert!(app
        .world()
        .get::<EditableText>(first)
        .unwrap()
        .pending_edits
        .is_empty());
    assert_eq!(
        app.world()
            .get::<EditableText>(second)
            .unwrap()
            .pending_edits,
        [TextEdit::ImeCommit {
            value: "日本".into()
        }]
    );
}
