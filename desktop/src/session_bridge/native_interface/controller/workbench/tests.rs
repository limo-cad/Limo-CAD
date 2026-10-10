use super::*;
use crate::native_viewport::ui::HudAxisMark;
use crate::session_bridge::native_interface::tests::Fixture;
mod workspace;

#[test]
fn sketch_palette_retires_the_dial_and_restores_it_after_finish() {
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let front = world
        .spawn((Node::default(), InterfaceControl::button("view", "Front")))
        .id();
    bind_command(world, front, NativeCommand::Orient(ViewDirection::Front)).unwrap();
    let controls = HashMap::from([("front".into(), front)]);
    let mut state = Workbench::default();
    for sketch in [false, true, false] {
        state.sketch = sketch;
        state.widgets.begin();
        viewport::synchronize(world, camera, &controls, 1360., 860., 232., &mut state).unwrap();
        state.widgets.finish(world);
        assert_eq!(state.dial.is_some(), !sketch);
        assert_eq!(state.axes.is_some(), !sketch);
        assert_eq!(state.widgets.entity("dial-card").is_some(), !sketch);
        assert_eq!(
            world.get::<InterfaceControl>(front).unwrap().visible,
            !sketch
        );
        assert!(state.widgets.entity("navigation").is_some());
        assert_eq!(
            world.query::<&HudAxisMark>().iter(world).count(),
            if sketch { 0 } else { 36 }
        );
    }
}

#[test]
fn workspace_switcher_keeps_its_caption_at_every_width_and_disables_other_workspaces_in_sketch() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let mut state = Workbench {
        sketch: true,
        menu: Some("workspace".into()),
        ..default()
    };
    let mut retained = None;
    for width in [1440., 1360., 1024., 1440.] {
        state.widgets.begin();
        ribbon_menu::synchronize(
            world,
            camera,
            &HashMap::new(),
            width,
            true,
            &services,
            &mut state,
        )
        .unwrap();
        state.widgets.finish(world);
        let entity = state.widgets.entity("workspace").unwrap();
        assert_eq!(*retained.get_or_insert(entity), entity);
        let label = world
            .get::<Children>(entity)
            .unwrap()
            .iter()
            .find(|child| world.get::<Text>(*child).is_some())
            .unwrap();
        assert_eq!(world.get::<Node>(label).unwrap().width, px(100.));
        assert_eq!(
            world
                .get::<interface_shell::InterfaceCaption>(entity)
                .unwrap()
                .0,
            "Solid Modeling"
        );
        assert!(state.widgets.entity("workspace-sketch").is_some());
        assert!(state.widgets.entity("workspace-sketch-badge").is_some());
        for name in ["Drawing", "Manufacture"] {
            let entry = world
                .query::<&InterfaceControl>()
                .iter(world)
                .find(|c| c.label == name)
                .unwrap();
            assert!(entry.disabled);
        }
        assert!(state.widgets.entity("workspace-check-0").is_some());
    }
}

#[test]
fn workbench_history_epoch_keeps_workspace_but_retires_transient_state() {
    let original = DocumentContext {
        window_id: "main".into(),
        document_id: "tab-a".into(),
        epoch: 1,
    };
    let mut workbench = Workbench {
        owner: Some(original.clone()),
        workspace: Workspace::Cam,
        menu: Some("workspace".into()),
        navigation: NavigationTool::Pan,
        paper_labels: vec![drawing_paper::Label {
            text: "stale".into(),
            ..default()
        }],
        ..default()
    };
    let restored = DocumentContext {
        epoch: 2,
        ..original.clone()
    };
    workbench.refresh_owner(&restored);
    assert_eq!(workbench.workspace, Workspace::Cam);
    assert_eq!(workbench.owner.as_ref(), Some(&restored));
    assert!(workbench.menu.is_none());
    assert_eq!(workbench.navigation, NavigationTool::Select);
    assert!(workbench.paper_key.is_none() && workbench.paper_labels.is_empty());
    workbench.refresh_owner(&DocumentContext {
        document_id: "tab-b".into(),
        ..restored
    });
    assert_eq!(workbench.workspace, Workspace::Solid);
}

#[test]
fn native_ribbon_menus_retain_disabled_commands_and_navigation_toggles() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let create_sketch = world
        .spawn((
            Node::default(),
            InterfaceControl::button("sketch/create", "Create Sketch"),
        ))
        .id();
    bind_command(
        world,
        create_sketch,
        NativeCommand::Sketch(crate::native_editor::EditorCommand::Support(
            crate::native_editor::support::Command::Start,
        )),
    )
    .unwrap();
    let owner = fixture.owner();
    let mut state = Workbench::default();
    state.widgets.begin();
    ribbon_menu::synchronize(
        world,
        camera,
        &HashMap::new(),
        1200.,
        false,
        &services,
        &mut state,
    )
    .unwrap();
    world.insert_resource(state);
    execute(world, &Command::Menu("refine".into())).unwrap();
    assert_eq!(modal(world), Some("workbench-menu"));
    let mut state = world.remove_resource::<Workbench>().unwrap();
    state.widgets.begin();
    ribbon_menu::synchronize(
        world,
        camera,
        &HashMap::new(),
        1200.,
        false,
        &services,
        &mut state,
    )
    .unwrap();
    let draft = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "Draft")
        .unwrap();
    assert!(draft.disabled);
    assert_eq!(draft.role, "menuitem");
    assert_eq!(draft.modal_scope.as_deref(), Some("workbench-menu"));
    state.owner = Some(owner.clone());
    world.insert_resource(state);
    escape(world);
    assert_eq!(modal(world), None);
    execute(world, &Command::Menu("workspace".into())).unwrap();
    let mut state = world.remove_resource::<Workbench>().unwrap();
    state.widgets.begin();
    ribbon_menu::synchronize(
        world,
        camera,
        &HashMap::new(),
        1200.,
        false,
        &services,
        &mut state,
    )
    .unwrap();
    let drawing = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "Drawing")
        .unwrap();
    assert!(!drawing.disabled);
    assert_eq!(drawing.role, "menuitem");
    let cam = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "Manufacture")
        .unwrap();
    assert!(!cam.disabled);
    world.insert_resource(state);
    execute(world, &Command::Workspace(Workspace::Drawing)).unwrap();
    assert_eq!(workspace(world), Workspace::Drawing);
    let mut state = world.remove_resource::<Workbench>().unwrap();
    state.widgets.begin();
    ribbon_menu::synchronize(
        world,
        camera,
        &HashMap::new(),
        1200.,
        false,
        &services,
        &mut state,
    )
    .unwrap();
    let sheet = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "New Sheet")
        .unwrap();
    assert!(!sheet.disabled);
    assert!(
        !world
            .get::<InterfaceControl>(create_sketch)
            .unwrap()
            .visible
    );
    let delete = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "Delete sheet")
        .unwrap();
    assert!(delete.disabled);
    let status = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "No sheet")
        .unwrap();
    assert!(status.disabled);
    let front = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "Front View")
        .unwrap();
    assert!(front.disabled);
    let iso = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "Isometric")
        .unwrap();
    assert!(iso.disabled);
    let note = world
        .query::<&InterfaceControl>()
        .iter(world)
        .find(|c| c.label == "Note")
        .unwrap();
    assert!(note.disabled);
    state.workspace = Workspace::Cam;
    world
        .get_mut::<InterfaceControl>(create_sketch)
        .unwrap()
        .visible = true;
    ribbon_menu::synchronize(
        world,
        camera,
        &HashMap::new(),
        1200.,
        false,
        &services,
        &mut state,
    )
    .unwrap();
    assert!(
        !world
            .get::<InterfaceControl>(create_sketch)
            .unwrap()
            .visible
    );
    world.insert_resource(state);
    execute(world, &Command::Navigation(NavigationTool::Pan)).unwrap();
    assert_eq!(navigation(world), NavigationTool::Pan);
    execute(world, &Command::Navigation(NavigationTool::Pan)).unwrap();
    assert_eq!(navigation(world), NavigationTool::Select);
}
