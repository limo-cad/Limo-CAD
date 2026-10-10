use super::*;
use crate::{app_preferences::Locale, session_bridge::native_interface::tests::Fixture};
use bevy::{ecs::schedule::Schedule, text::EditableText};

fn panel(fixture: &Fixture) -> (App, NativeServices, NativeInterfaceHandle) {
    let (mut app, services, handle) = super::super::tests::setup(fixture);
    app.add_schedule(Schedule::new(Update))
        .init_resource::<Assets<Image>>()
        .init_resource::<ViewportUiAssets>()
        .init_resource::<bevy::input_focus::InputFocus>();
    fields::install(&mut app);
    app.world_mut().spawn(InterfaceCamera);
    (app, services, handle)
}

fn entity(world: &World, key: &str) -> Entity {
    world.resource::<Widgets>().controls[key].0
}

#[test]
fn file_chrome_translates_by_key_without_rebinding_actions_or_translating_document_names() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let name = "Save — Café 原始项目";
    fixture.rename(&fixture.owner(), name).unwrap();
    let model = fixture.engine.engine_call("project_export_model", "");
    let (mut app, services, _) = panel(&fixture);
    let world = app.world_mut();
    world.resource_mut::<Files>().menu = true;
    synchronize(world, &services, &fixture.owner(), 1360., 860.).unwrap();
    let originals: Vec<_> = [
        "file",
        "new",
        "file-item-0",
        "file-item-4",
        "file-item-13",
        "file-item-14",
        "file-item-16",
    ]
    .into_iter()
    .map(|key| {
        let entity = entity(world, key);
        (
            key,
            entity,
            world.get::<NativeCommandBinding>(entity).unwrap().clone(),
        )
    })
    .collect();
    let scripts = world
        .resource::<Widgets>()
        .chrome
        .entity("scripts")
        .unwrap();
    let tab = entity(world, &format!("tab-{}", fixture.owner().document_id));
    for locale in [Locale::ZhCn, Locale::Es, Locale::De, Locale::En] {
        localization::set_locale(world, locale);
        synchronize(world, &services, &fixture.owner(), 1360., 860.).unwrap();
        for (key, original_entity, original_binding) in &originals {
            let current = entity(world, key);
            assert_eq!(current, *original_entity);
            let binding = world.get::<NativeCommandBinding>(current).unwrap();
            assert_eq!(binding.generation, original_binding.generation);
            assert_eq!(binding.command, original_binding.command);
        }
        for (key, translation) in [
            ("file", "file.menu"),
            ("new", "topbar.newDesign"),
            ("file-item-0", "file.open"),
            ("file-item-4", "file.rename"),
            ("file-item-13", "file.exportDrawingDxf"),
            ("file-item-14", "file.exportDrawingSvg"),
            ("file-item-16", "topbar.settings"),
            ("file-backdrop", "file.closeMenu"),
        ] {
            assert_eq!(
                world
                    .get::<InterfaceControl>(entity(world, key))
                    .unwrap()
                    .label,
                dictionary::translate(locale, translation)
            );
        }
        assert_eq!(
            world.resource::<Widgets>().chrome.entity("scripts"),
            Some(scripts)
        );
        assert_eq!(
            world.get::<InterfaceControl>(scripts).unwrap().label,
            dictionary::translate(locale, "topbar.scripts")
        );
        assert_eq!(world.get::<InterfaceControl>(tab).unwrap().label, name);
        let footer = world
            .resource::<Widgets>()
            .chrome
            .entity("file-footer")
            .unwrap();
        assert_eq!(
            world.get::<Text>(footer).unwrap().0,
            dictionary::translate(locale, "file.zipHint")
        );
    }
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        model
    );
}

#[test]
fn language_repaint_updates_dialog_text_and_preserves_uncommitted_native_rename_buffer() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let saved_name = "Settings — material 零件";
    fixture.rename(&fixture.owner(), saved_name).unwrap();
    let model = fixture.engine.engine_call("project_export_model", "");
    let (mut app, services, handle) = panel(&fixture);
    execute(
        app.world_mut(),
        &handle,
        &services,
        &fixture.owner(),
        FileCommand::Rename,
    )
    .unwrap();
    synchronize(app.world_mut(), &services, &fixture.owner(), 1360., 860.).unwrap();
    app.world_mut().run_schedule(Update);
    let field = entity(app.world(), "rename-value");
    let binding = app
        .world()
        .get::<NativeCommandBinding>(field)
        .unwrap()
        .clone();
    let draft = "Uncommitted — Café 新名字";
    app.world_mut()
        .get_mut::<EditableText>(field)
        .unwrap()
        .editor
        .set_text(draft);
    for locale in [Locale::ZhCn, Locale::De, Locale::En] {
        localization::set_locale(app.world_mut(), locale);
        synchronize(app.world_mut(), &services, &fixture.owner(), 1360., 860.).unwrap();
        app.world_mut().run_schedule(Update);
        let world = app.world();
        assert_eq!(entity(world, "rename-value"), field);
        assert_eq!(world.get::<EditableText>(field).unwrap().value(), draft);
        let current = world.get::<NativeCommandBinding>(field).unwrap();
        assert_eq!(current.generation, binding.generation);
        assert_eq!(current.command, binding.command);
        let control = world.get::<InterfaceControl>(field).unwrap();
        assert_eq!(
            control.label,
            dictionary::translate(locale, "file.renamePrompt")
        );
        assert!(matches!(&control.field, Field::Text { value, .. } if value == saved_name));
        let title = dictionary::translate(locale, "file.rename");
        assert!(world
            .resource::<Widgets>()
            .decoration
            .iter()
            .any(|entity| world
                .get::<Text>(*entity)
                .is_some_and(|text| text.0 == title)));
        assert_eq!(
            world
                .get::<InterfaceControl>(entity(world, "cancel-file"))
                .unwrap()
                .label,
            dictionary::translate(locale, "file.cancel")
        );
    }
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        model
    );
}
