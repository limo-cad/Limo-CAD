use super::*;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "limo-cad-script-editor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn authored() -> &'static str {
    "// Preserve authorship and relative includes.\n{\"version\":1,\"name\":\"Authored source\",\"includes\":[\"part.collection.jsonc\"],\"steps\":[{\"note\":\"Root\"}]}"
}
fn fixture(directory: &Directory) -> World {
    let path = directory.0.join("source.limo.jsonc");
    std::fs::write(&path, authored()).unwrap();
    std::fs::write(
        directory.0.join("part.collection.jsonc"),
        r#"{"steps":[{"note":"Original include"}]}"#,
    )
    .unwrap();
    let mut world = World::new();
    initialize(
        &mut world,
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    world
        .resource_mut::<Files>()
        .script
        .accept(inspect(path).unwrap())
        .unwrap();
    world
}
fn validate_now(world: &mut World) -> Result<(), String> {
    let state = &mut world.resource_mut::<Files>().script;
    state.advance()?;
    state.validated = false;
    let loaded = inspect_source(state.source_path.clone(), &state.source)?;
    let generation = state.generation;
    apply(state, Change::Validated { generation, loaded })
}

#[test]
fn native_script_editor_validation_uses_exact_authored_draft_without_marking_it_saved() {
    let directory = Directory::new();
    let mut world = fixture(&directory);
    let initial = world.resource::<Files>().script.generation;
    let original = world.resource::<Files>().script.selected(initial).unwrap();
    assert_eq!(world.resource::<Files>().script.source, authored());
    let edited = authored().replace("Authored source", "Edited source");
    edit_source(&mut world, &ControlInput::SetValue(edited.clone())).unwrap();
    let state = &world.resource::<Files>().script;
    assert!(state.dirty());
    assert!(state.selected(initial).is_err());
    assert!(state.selected(state.generation).is_err());
    assert!(replace_ready(&mut world).is_err());
    validate_now(&mut world).unwrap();
    let state = &world.resource::<Files>().script;
    assert!(
        state.dirty(),
        "Validation must not mark edited source saved"
    );
    let validated = state.selected(state.generation).unwrap();
    assert_eq!(validated.authored(), edited);
    assert_eq!(validated.name, "Edited source");
    assert!(validated.source().contains("Original include"));
    assert!(!validated.source().contains("includes"));
    std::fs::write(directory.0.join("source.limo.jsonc"), "external root edit").unwrap();
    std::fs::write(
        directory.0.join("part.collection.jsonc"),
        "external include edit",
    )
    .unwrap();
    assert!(
        state.selected(state.generation).is_ok(),
        "Run uses the exact validated snapshot"
    );
    assert_eq!(original.authored(), authored());
    assert!(original.source().contains("Original include"));
}

#[test]
fn native_script_editor_invalid_and_unsafe_drafts_cannot_use_a_previous_validation() {
    let directory = Directory::new();
    let mut world = fixture(&directory);
    for source in [
        "{ unfinished source",
        r#"{"version":1,"name":"Escape","includes":["../escape.collection.jsonc"],"steps":[{"note":"No"}]}"#,
        r#"{"version":1,"name":"Transport","steps":[{"call":{"group":"document/session","operation":"cad_interface","arguments":{}}}]}"#,
    ] {
        edit_source(&mut world, &ControlInput::SetValue(source.into())).unwrap();
        assert!(validate_now(&mut world).is_err());
        let state = &world.resource::<Files>().script;
        assert!(state.selected(state.generation).is_err());
        assert_eq!(state.source, source, "Parse failure preserves the draft");
        assert!(state.dirty());
    }
    discard(&mut world).unwrap();
    assert_eq!(world.resource::<Files>().script.source, authored());
    assert!(!world.resource::<Files>().script.dirty());
    assert!(replace_ready(&mut world).is_ok());
    validate_now(&mut world).unwrap();
    let state = &world.resource::<Files>().script;
    assert!(state.selected(state.generation).is_ok());
}

#[test]
fn native_script_editor_save_preserves_invalid_authored_source_and_rebases_includes_explicitly() {
    let directory = Directory::new();
    let mut world = fixture(&directory);
    let destination = directory.0.join("elsewhere");
    std::fs::create_dir(&destination).unwrap();
    let path = destination.join("saved.limo.jsonc");
    let draft = format!("{}\n// unfinished comment\n{{", authored());
    edit_source(&mut world, &ControlInput::SetValue(draft.clone())).unwrap();
    let maximum = world
        .resource::<Files>()
        .script
        .loaded
        .as_ref()
        .unwrap()
        .maximum;
    write_source(&path, &draft, maximum).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), draft);
    assert_eq!(
        std::fs::read_to_string(directory.0.join("source.limo.jsonc")).unwrap(),
        authored()
    );
    assert!(
        !destination.join("part.collection.jsonc").exists(),
        "Save must never copy or flatten included files"
    );
    {
        let state = &mut world.resource_mut::<Files>().script;
        let generation = state.generation;
        apply(
            state,
            Change::Saved {
                generation,
                path: path.clone(),
                source: draft,
            },
        )
        .unwrap();
        assert!(!state.dirty());
        assert_eq!(state.source_path.as_ref(), Some(&path));
        assert!(state.selected(state.generation).is_err());
    }
    edit_source(&mut world, &ControlInput::SetValue(authored().into())).unwrap();
    assert!(
        validate_now(&mut world).is_err(),
        "Includes must resolve in the new save directory"
    );
    std::fs::write(
        destination.join("part.collection.jsonc"),
        r#"{"steps":[{"note":"Saved directory include"}]}"#,
    )
    .unwrap();
    validate_now(&mut world).unwrap();
    let state = &world.resource::<Files>().script;
    let loaded = state.selected(state.generation).unwrap();
    assert!(loaded.source().contains("Saved directory include"));
    assert!(!loaded.source().contains("Original include"));
    assert!(write_source(&destination.join("wrong.json"), "bad", maximum).is_err());
    assert!(write_source(&path, "too long", 1).is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        format!("{}\n// unfinished comment\n{{", authored())
    );
}

#[test]
fn native_script_editor_stale_completions_and_rejected_native_buffers_cannot_enable_run() {
    let directory = Directory::new();
    let mut world = fixture(&directory);
    let state = &world.resource::<Files>().script;
    let generation = state.generation;
    let loaded = inspect_source(state.source_path.clone(), &state.source).unwrap();
    edit_source(
        &mut world,
        &ControlInput::SetValue(authored().replace("Root", "Changed")),
    )
    .unwrap();
    assert!(apply(
        &mut world.resource_mut::<Files>().script,
        Change::Validated { generation, loaded }
    )
    .is_err());
    let stale_save = Change::Saved {
        generation,
        path: directory.0.join("old.limo.jsonc"),
        source: authored().into(),
    };
    assert!(apply(&mut world.resource_mut::<Files>().script, stale_save).is_err());
    assert!(world.resource::<Files>().script.dirty());
    validate_now(&mut world).unwrap();
    {
        let state = &mut world.resource_mut::<Files>().script;
        let generation = state.generation;
        let wrong_path = inspect_source(
            Some(directory.0.join("different.limo.jsonc")),
            &state.source,
        )
        .unwrap();
        assert!(
            apply(
                state,
                Change::Validated {
                    generation,
                    loaded: wrong_path
                }
            )
            .is_err(),
            "Validation must match the include/source path as well as the edit generation"
        );
    }
    let entity = world
        .spawn(fields::limits::ByteLimit {
            maximum: 16 * 1024 * 1024,
            rejected: Some("Rejected native insertion".into()),
        })
        .id();
    world.resource_mut::<Files>().script.source_entity = Some(entity);
    assert!(source_ready(&world).is_err());
    retain_source_error(&mut world);
    world.despawn(entity);
    assert!(
        source_ready(&world).is_err(),
        "Closing source view cannot hide a rejected edit"
    );
    let state = &world.resource::<Files>().script;
    assert!(state.selected(state.generation).is_err());
    let maximum = state.loaded.as_ref().unwrap().maximum;
    assert!(edit_source(&mut world, &ControlInput::SetValue(" ".repeat(maximum + 1))).is_err());
    assert!(source_ready(&world).is_err());
    discard(&mut world).unwrap();
    assert!(source_ready(&world).is_ok());
    validate_now(&mut world).unwrap();
}

#[test]
fn native_script_editor_preserves_rejected_native_draft_and_rebinds_on_explicit_discard() {
    use super::super::super::super::chrome;
    use bevy::text::EditableText;
    let directory = Directory::new();
    let mut world = fixture(&directory);
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn_empty().id();
    let theme = crate::native_viewport::ui::theme(&world);
    let mut widgets = chrome::Widgets::default();
    paint_source(&mut world, camera, &mut widgets, 1280., 800., theme, false).unwrap();
    let entity = world.resource::<Files>().script.source_entity.unwrap();
    let original_binding = world.get::<InterfaceControl>(entity).unwrap().binding;
    assert_eq!(
        world.get::<InterfaceControl>(entity).unwrap().role,
        "multiline_textbox"
    );
    assert!(world.get::<EditableText>(entity).unwrap().allow_newlines);
    let native_draft = authored().replace("Root", "Native draft before rejected insertion");
    world
        .get_mut::<EditableText>(entity)
        .unwrap()
        .editor
        .set_text(&native_draft);
    world
        .get_mut::<fields::limits::ByteLimit>(entity)
        .unwrap()
        .rejected = Some("Too large".into());
    poll(&mut world);
    assert_eq!(
        world.resource::<Files>().script.source,
        authored(),
        "Repainting a rejected edit must not reset the active field's native Undo history"
    );
    assert_eq!(
        world
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        native_draft
    );
    show_source(&mut world).unwrap();
    assert_eq!(world.resource::<Files>().script.source, native_draft);
    assert!(world.resource::<Files>().script.dirty());
    assert!(source_ready(&world).is_err());
    discard(&mut world).unwrap();
    paint_source(&mut world, camera, &mut widgets, 1280., 800., theme, false).unwrap();
    assert_eq!(world.resource::<Files>().script.source_entity, Some(entity));
    let control = world.get::<InterfaceControl>(entity).unwrap();
    assert_ne!(control.binding, original_binding,
        "Rebind tells the existing native field adapter to discard its stale local buffer and history");
    assert!(matches!(&control.field, Field::Text { value, .. } if value == authored()));
    assert!(source_ready(&world).is_ok());
}
