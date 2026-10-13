use super::*;

fn owner(window: &str, document: &str, epoch: u64) -> DocumentContext {
    DocumentContext {
        window_id: window.into(),
        document_id: document.into(),
        epoch,
    }
}

#[test]
fn workbench_restores_each_documents_workspace_without_transient_presentation() {
    let first = owner("main", "first", 1);
    let second = owner("main", "second", 1);
    let foreign = owner("other-window", "first", 1);
    let mut state = Workbench::default();
    state.refresh_owner(&first);
    state.workspace = Workspace::Drawing;
    state.menu = Some("workspace".into());
    state.navigation = NavigationTool::Pan;
    state.paper_labels = vec![drawing_paper::Label {
        text: "stale".into(),
        ..default()
    }];
    state.refresh_owner(&second);
    assert_eq!(state.workspace, Workspace::Solid);
    assert!(state.menu.is_none());
    assert_eq!(state.navigation, NavigationTool::Select);
    assert!(state.paper_labels.is_empty());
    state.workspace = Workspace::Cam;

    state.refresh_owner(&owner("main", "first", 2));
    assert_eq!(state.workspace, Workspace::Drawing);
    state.refresh_owner(&foreign);
    assert_eq!(
        state.workspace,
        Workspace::Solid,
        "Window ownership is part of the key"
    );
    state.refresh_owner(&second);
    assert_eq!(state.workspace, Workspace::Cam);
    state.refresh_owner(&first);
    assert_eq!(state.workspace, Workspace::Drawing);
}

#[test]
fn workbench_retires_exact_closed_document_without_resaving_it() {
    let first = owner("main", "first", 1);
    let second = owner("main", "second", 1);
    let foreign = owner("other-window", "first", 1);
    let mut world = World::new();
    observe_document(&mut world, &first);
    execute(&mut world, &Command::Workspace(Workspace::Drawing)).unwrap();
    observe_document(&mut world, &second);
    execute(&mut world, &Command::Workspace(Workspace::Cam)).unwrap();
    observe_document(&mut world, &foreign);
    execute(&mut world, &Command::Workspace(Workspace::Drawing)).unwrap();
    retire_document(&mut world, &first);
    assert_eq!(workspace(&world), Workspace::Drawing);
    assert_eq!(world.resource::<Workbench>().owner.as_ref(), Some(&foreign));

    retire_document(&mut world, &foreign);
    assert_eq!(workspace(&world), Workspace::Solid);
    observe_document(&mut world, &second);
    assert_eq!(workspace(&world), Workspace::Cam);
    let state = world.resource::<Workbench>();
    assert!(!state
        .workspaces
        .contains_key(&(first.window_id.clone(), first.document_id.clone())));
    assert!(!state
        .workspaces
        .contains_key(&(foreign.window_id.clone(), foreign.document_id.clone())));
    assert_eq!(state.workspaces.len(), 1);
}
