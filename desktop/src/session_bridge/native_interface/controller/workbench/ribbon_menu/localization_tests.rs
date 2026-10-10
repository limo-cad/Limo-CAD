use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

#[test]
fn catalog_uses_explicit_keys_and_keeps_visible_fallback_for_missing_translations() {
    let entry = json!({"id":"extrude", "labelKey":"ribbon.solid.extrude"});
    assert_eq!(label(Locale::En, &entry), "Extrude");
    for locale in [Locale::ZhCn, Locale::Es, Locale::De] {
        assert_eq!(
            label(locale, &entry),
            dictionary::translate(locale, "ribbon.solid.extrude")
        );
        assert_ne!(label(locale, &entry), "Extrude");
        let absent = json!({"id":"untranslated", "labelKey":"ribbon.future.command"});
        assert_eq!(label(locale, &absent), "ribbon.future.command");
    }
    assert_eq!(key("patternRectangular"), "solid-rectangular-pattern");
    assert_eq!(workspace_name(Workspace::Cam), "CAM");
    assert_eq!(
        dictionary::translate(Locale::En, workspace_label_key(Workspace::Cam)),
        "Manufacture"
    );
}

#[test]
fn changing_locale_retains_real_command_identity_and_document_names() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    fixture
        .rename(&fixture.owner(), "Extrude — 未翻译的项目")
        .unwrap();
    let model = fixture.engine.engine_call("project_export_model", "");
    let revision = fixture.engine.geometry_revision();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let mut originals = Widgets::default();
    let sketch = originals
        .button(
            world,
            camera,
            "sketch",
            InterfaceControl::button("sketch/draw", "Already translated"),
            None,
            NativeCommand::Sketch(crate::native_editor::EditorCommand::Support(
                crate::native_editor::support::Command::Start,
            )),
            ribbon::node(0., 0., 48.),
            None,
            30,
        )
        .unwrap();
    ribbon::decorate(world, sketch, Icon::Sketch);
    let extrude = originals
        .button(
            world,
            camera,
            "extrude",
            InterfaceControl::button("solid/build", "Extrude"),
            None,
            NativeCommand::ClearSelection,
            ribbon::node(0., 0., 48.),
            None,
            30,
        )
        .unwrap();
    ribbon::decorate(world, extrude, Icon::Extrude);
    let decoy = world
        .spawn(InterfaceControl::button(
            "document/session",
            "Create Sketch",
        ))
        .id();
    bind_command(world, decoy, NativeCommand::Workbench(Command::Dismiss)).unwrap();
    let controls = HashMap::from([("extrude".to_owned(), extrude)]);
    let sketch_binding = world.get::<NativeCommandBinding>(sketch).unwrap().clone();
    let extrude_binding = world.get::<NativeCommandBinding>(extrude).unwrap().clone();
    let mut state = Workbench::default();
    let mut retained_group = None;
    let mut retained_reference = None;
    for locale in [Locale::En, Locale::ZhCn, Locale::Es, Locale::De, Locale::En] {
        localization::set_locale(world, locale);
        state.widgets.begin();
        synchronize(
            world, camera, &controls, 1900., false, &services, &mut state,
        )
        .unwrap();
        state.widgets.finish(world);
        assert_eq!(source(world, &controls, "createSketch"), Some(sketch));
        assert_eq!(
            world.get::<InterfaceControl>(sketch).unwrap().label,
            dictionary::translate(locale, "ribbon.solid.createSketch")
        );
        assert_eq!(
            world.get::<InterfaceControl>(extrude).unwrap().label,
            dictionary::translate(locale, "ribbon.solid.extrude")
        );
        assert_eq!(
            world.get::<InterfaceControl>(decoy).unwrap().label,
            "Create Sketch"
        );
        for (entity, original) in [(sketch, &sketch_binding), (extrude, &extrude_binding)] {
            let binding = world.get::<NativeCommandBinding>(entity).unwrap();
            assert_eq!(binding.generation, original.generation);
            assert_eq!(binding.command, original.command);
        }
        let group = state.widgets.entity("group-build").unwrap();
        let reference = state.widgets.entity("tool-constructionVisibility").unwrap();
        assert_eq!(*retained_group.get_or_insert(group), group);
        assert_eq!(*retained_reference.get_or_insert(reference), reference);
        assert_eq!(
            world.get::<InterfaceControl>(group).unwrap().label,
            dictionary::translate(locale, "ribbon.panels.build")
        );
        assert_eq!(
            world.get::<InterfaceControl>(reference).unwrap().label,
            dictionary::translate(locale, "ribbon.solid.constructionVisibility")
        );
    }
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        model
    );
    assert_eq!(fixture.engine.geometry_revision(), revision);

    state.menu = Some("workspace".into());
    state.workspace = Workspace::Cam;
    let mut retained_rows = Vec::new();
    for locale in [Locale::En, Locale::ZhCn, Locale::De] {
        localization::set_locale(world, locale);
        state.widgets.begin();
        menu(
            (world, camera),
            1900.,
            4.,
            &[],
            &controls,
            &services,
            &mut state,
        )
        .unwrap();
        state.widgets.finish(world);
        for (index, workspace) in [Workspace::Solid, Workspace::Drawing, Workspace::Cam]
            .into_iter()
            .enumerate()
        {
            let entity = state.widgets.entity(&format!("menu-row-{index}")).unwrap();
            if retained_rows.len() <= index {
                retained_rows.push(entity);
            }
            assert_eq!(retained_rows[index], entity);
            let control = world.get::<InterfaceControl>(entity).unwrap();
            assert_eq!(
                control.label,
                dictionary::translate(locale, workspace_label_key(workspace))
            );
            assert_eq!(
                control.selected,
                (workspace == Workspace::Cam).then_some(true)
            );
            assert_eq!(
                world.get::<NativeCommandBinding>(entity).unwrap().command,
                NativeCommand::Workbench(Command::Workspace(workspace))
            );
        }
    }
    state.menu = None;
    state.workspace = Workspace::Drawing;
    for locale in [Locale::En, Locale::De] {
        localization::set_locale(world, locale);
        state.widgets.begin();
        synchronize(
            world, camera, &controls, 1900., false, &services, &mut state,
        )
        .unwrap();
        state.widgets.finish(world);
        assert_eq!(
            world
                .get::<InterfaceControl>(state.widgets.entity("drawing-new-sheet").unwrap())
                .unwrap()
                .label,
            dictionary::translate(locale, "ribbon.drawing.newSheet")
        );
        assert_eq!(
            world
                .get::<InterfaceControl>(state.widgets.entity("drawing-front").unwrap())
                .unwrap()
                .label,
            dictionary::translate(locale, "ribbon.drawing.front")
        );
    }
    state.menu = Some("drawing-dimensions".into());
    localization::set_locale(world, Locale::De);
    state.widgets.begin();
    synchronize(
        world, camera, &controls, 1900., false, &services, &mut state,
    )
    .unwrap();
    state.widgets.finish(world);
    assert_eq!(
        world
            .get::<InterfaceControl>(state.widgets.entity("menu-row-9").unwrap())
            .unwrap()
            .label,
        dictionary::translate(Locale::De, "ribbon.drawing.centerLineBetweenEdges")
    );
    assert_eq!(
        world
            .get::<InterfaceControl>(state.widgets.entity("menu-row-20").unwrap())
            .unwrap()
            .label,
        dictionary::translate(Locale::De, "drawing.workspace.reassociateReferences")
    );
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        model
    );
}
