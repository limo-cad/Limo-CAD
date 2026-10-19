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
    let loaded = state
        .loaded
        .as_ref()
        .unwrap()
        .artifact
        .inspect(state.source_path.clone(), &state.source)?;
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
                artifact: Artifact::Text,
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
        artifact: Artifact::Text,
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

fn project_fixture(directory: &Directory) -> (World, Vec<u8>) {
    use limo_cad_project_file::{ProjectArchive, ReplayPackage, SaveMetadata};
    let mut archive = ProjectArchive::new(
        r#"{"format":"limo-cad-project","schema_version":15}"#.into(),
        SaveMetadata {
            application_version: "test",
            saved_at: "2026-10-17T00:00:00Z",
        },
    )
    .unwrap();
    archive
        .set_intent("intent/future.cbor", Some(vec![0, 255, 7]))
        .unwrap();
    let bytes = archive.encode().unwrap();
    let source = r#"{"version":2,"name":"Retained","starting_state":"project","steps":[{"note":"Original"}]}"#;
    let path = directory.0.join("source.limo.replay.cbor");
    ReplayPackage::new(source.into(), bytes.clone())
        .unwrap()
        .write_file_new(&path)
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
    (world, bytes)
}

#[test]
fn native_package_edit_validate_save_retains_baseline_and_uses_memory_snapshot() {
    use limo_cad_project_file::ReplayPackage;
    let directory = Directory::new();
    let (mut world, original) = project_fixture(&directory);
    let loaded = world
        .resource::<Files>()
        .script
        .loaded
        .as_ref()
        .unwrap()
        .clone();
    let draft = loaded.authored().replace("Original", "Edited");
    edit_source(&mut world, &ControlInput::SetValue(draft.clone())).unwrap();
    // External replacement must not mix a new baseline with the editor's draft.
    std::fs::write(
        directory.0.join("source.limo.replay.cbor"),
        b"external replacement",
    )
    .unwrap();
    validate_now(&mut world).unwrap();
    let path = directory.0.join("saved.limo.replay.cbor");
    {
        let state = &mut world.resource_mut::<Files>().script;
        let selected = state.selected(state.generation).unwrap();
        assert!(loaded.artifact.same_baseline(&selected.artifact));
        assert!(state.dirty());
        let prepared = selected
            .artifact
            .prepare(selected.source(), false)
            .unwrap()
            .unwrap();
        let (_, archive, _, _) = prepared.into_parts();
        assert_eq!(archive.encode().unwrap(), original);
        let artifact = selected
            .artifact
            .save(&path, &draft, selected.maximum)
            .unwrap();
        let reopened = ReplayPackage::read_file(&path).unwrap();
        assert_eq!(reopened.script(), draft);
        assert_eq!(reopened.project_bytes(), original);
        let generation = state.generation;
        apply(
            state,
            Change::Saved {
                generation,
                path: path.clone(),
                source: draft,
                artifact,
            },
        )
        .unwrap();
        assert!(!state.dirty());
        assert!(!state.validated);
    }
    validate_now(&mut world).unwrap();
    let state = &world.resource::<Files>().script;
    let selected = state.selected(state.generation).unwrap();
    let (_, saved) = ReplayPackage::read_file_with_identity(&path).unwrap();
    assert_eq!(selected.inspection["saved_package_identity"], json!(saved));
    assert_ne!(selected.inspection["opened_package_identity"], json!(saved));
}

#[test]
fn native_package_wrong_baseline_and_stale_saves_cannot_change_editor_state() {
    let directory = Directory::new();
    let (mut world, _) = project_fixture(&directory);
    let state = &mut world.resource_mut::<Files>().script;
    let original = state.loaded.as_ref().unwrap().clone();
    // Identical file bytes reopened separately are a different selected baseline.
    let replacement = inspect(state.source_path.clone().unwrap()).unwrap();
    assert!(!original.artifact.same_baseline(&replacement.artifact));
    let generation = state.generation;
    assert!(apply(
        state,
        Change::Validated {
            generation,
            loaded: replacement.clone()
        }
    )
    .is_err());
    let source = state.source.clone();
    assert!(apply(
        state,
        Change::Saved {
            generation,
            path: directory.0.join("wrong.limo.replay.cbor"),
            source,
            artifact: replacement.artifact
        }
    )
    .is_err());
    assert!(original
        .artifact
        .save(
            &directory.0.join("lossy.limo.jsonc"),
            &state.source,
            original.maximum
        )
        .is_err());
    assert!(!directory.0.join("lossy.limo.jsonc").exists());
    let snapshot = state.source.clone();
    state.source.push_str("\n// later edit");
    state.advance().unwrap();
    assert!(apply(
        state,
        Change::Saved {
            generation,
            path: directory.0.join("old.limo.replay.cbor"),
            source: snapshot,
            artifact: original.artifact.clone()
        }
    )
    .is_err());
    assert!(state.dirty());
    assert!(state
        .loaded
        .as_ref()
        .unwrap()
        .artifact
        .same_baseline(&original.artifact));
}

#[test]
fn invalid_package_draft_reopens_for_repair_without_enabling_run_or_losing_baseline() {
    use limo_cad_project_file::ReplayPackage;
    let directory = Directory::new();
    let (world, original) = project_fixture(&directory);
    let loaded = world
        .resource::<Files>()
        .script
        .loaded
        .as_ref()
        .unwrap()
        .clone();
    let path = directory.0.join("unfinished.limo.replay.cbor");
    let unfinished = "// unfinished authored draft\n{";
    loaded
        .artifact
        .save(&path, unfinished, loaded.maximum)
        .unwrap();
    let reopened = inspect(path.clone()).unwrap();
    assert_eq!(reopened.authored(), unfinished);
    assert!(reopened.source_diagnostic().is_some());
    assert!(reopened.counts.is_none());
    let mut recovery = World::new();
    initialize(
        &mut recovery,
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    recovery
        .resource_mut::<Files>()
        .script
        .accept(reopened)
        .unwrap();
    {
        let state = &recovery.resource::<Files>().script;
        assert!(state.editor_open);
        assert!(!state.dirty());
        assert!(!state.validated);
        assert!(state.selected(state.generation).is_err());
        assert!(state.status.is_some());
    }
    // Saving another unfinished snapshot keeps the baseline without validating.
    let artifact = recovery
        .resource::<Files>()
        .script
        .loaded
        .as_ref()
        .unwrap()
        .artifact
        .clone();
    let backup = directory.0.join("backup.limo.replay.cbor");
    artifact.save(&backup, unfinished, loaded.maximum).unwrap();
    assert_eq!(
        ReplayPackage::read_file(&backup).unwrap().project_bytes(),
        original
    );
    assert!(validate_now(&mut recovery).is_err());
    assert_eq!(recovery.resource::<Files>().script.source, unfinished);
    edit_source(
        &mut recovery,
        &ControlInput::SetValue(loaded.authored().to_owned()),
    )
    .unwrap();
    validate_now(&mut recovery).unwrap();
    let state = &recovery.resource::<Files>().script;
    let repaired = state.selected(state.generation).unwrap();
    assert!(repaired.source_diagnostic().is_none());
    assert_eq!(repaired.counts, Some((1, 0)));
    assert!(
        state.dirty(),
        "Validation does not mark the repaired draft saved"
    );
    let prepared = repaired
        .artifact
        .prepare(repaired.source(), false)
        .unwrap()
        .unwrap();
    let (_, retained, _, _) = prepared.into_parts();
    assert_eq!(retained.encode().unwrap(), original);
    assert_eq!(
        ReplayPackage::read_file(&path).unwrap().script(),
        unfinished
    );
}

#[test]
fn malformed_package_envelope_is_not_admitted_as_a_repairable_script() {
    let directory = Directory::new();
    let (mut world, _) = project_fixture(&directory);
    let original = world
        .resource::<Files>()
        .script
        .loaded
        .as_ref()
        .unwrap()
        .clone();
    let path = directory.0.join("corrupt.limo.replay.cbor");
    std::fs::write(&path, [255]).unwrap();
    assert!(inspect(path).is_err());
    let state = &mut world.resource_mut::<Files>().script;
    assert!(Arc::ptr_eq(state.loaded.as_ref().unwrap(), &original));
    // Even an accidentally set state flag cannot bypass an invalid Loaded object.
    let draft_path = directory.0.join("draft.limo.replay.cbor");
    original
        .artifact
        .save(&draft_path, "invalid", original.maximum)
        .unwrap();
    state.accept(inspect(draft_path).unwrap()).unwrap();
    state.validated = true;
    assert!(state.selected(state.generation).is_err());
}

#[test]
fn unsupported_version_and_catalog_calls_open_only_as_unvalidated_package_source() {
    let directory = Directory::new();
    let (world, original) = project_fixture(&directory);
    let loaded = world
        .resource::<Files>()
        .script
        .loaded
        .as_ref()
        .unwrap()
        .clone();
    for (index, source) in [
        r#"{"version":99,"name":"Future","starting_state":"project","steps":[]}"#,
        r#"{"version":2,"name":"Wrong group","starting_state":"project","steps":[{"call":{"group":"wrong/group","operation":"cad_set_document_name","arguments":{"name":"No"}}}]}"#,
    ].iter().enumerate() {
        let path = directory.0.join(format!("repair-{index}.limo.replay.cbor"));
        loaded.artifact.save(&path, source, loaded.maximum).unwrap();
        let reopened = inspect(path).unwrap();
        assert_eq!(reopened.authored(), *source);
        assert!(reopened.counts.is_none());
        assert!(reopened.artifact.prepare(source, false).is_err());
        let saved = directory.0.join(format!("retained-{index}.limo.replay.cbor"));
        reopened.artifact.save(&saved, source, reopened.maximum).unwrap();
        assert_eq!(limo_cad_project_file::ReplayPackage::read_file(&saved).unwrap().project_bytes(), original);
    }
}

#[test]
fn jsonc_repair_preserves_authorship_and_missing_include_remains_non_runnable() {
    let directory = Directory::new();
    let path = directory.0.join("unfinished.limo.jsonc");
    let unfinished = "// preserve this unfinished source\n{";
    std::fs::write(&path, unfinished).unwrap();
    let loaded = inspect(path.clone()).unwrap();
    assert!(loaded.counts.is_none());
    assert_eq!(loaded.authored(), unfinished);
    let mut world = World::new();
    initialize(
        &mut world,
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    world.resource_mut::<Files>().script.accept(loaded).unwrap();
    {
        let state = &world.resource::<Files>().script;
        assert!(state.editor_open && !state.validated);
        assert!(state.selected(state.generation).is_err());
        let backup = directory.0.join("backup.limo.jsonc");
        state
            .loaded
            .as_ref()
            .unwrap()
            .artifact
            .save(&backup, &state.source, limo_cad_mcp::MAX_SCRIPT_BYTES)
            .unwrap();
        assert_eq!(std::fs::read_to_string(backup).unwrap(), unfinished);
    }
    edit_source(&mut world, &ControlInput::SetValue(authored().into())).unwrap();
    assert!(
        validate_now(&mut world).is_err(),
        "Missing include is not bypassed by recovery"
    );
    assert_eq!(world.resource::<Files>().script.source, authored());
    std::fs::write(
        directory.0.join("part.collection.jsonc"),
        r#"{"steps":[{"note":"Recovered include"}]}"#,
    )
    .unwrap();
    validate_now(&mut world).unwrap();
    let state = &world.resource::<Files>().script;
    let repaired = state.selected(state.generation).unwrap();
    assert!(repaired.source().contains("Recovered include"));
    assert_eq!(repaired.authored(), authored());
    assert!(state.dirty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), unfinished);
}
