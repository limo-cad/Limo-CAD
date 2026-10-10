use super::super::super::drawing_navigation_input;
use super::*;
use crate::native_viewport::winit_host::NativeHostInput;
use bevy::{
    input::{
        mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
        ButtonState,
    },
    window::{CursorMoved, WindowEvent},
};

fn input(owner: &DocumentContext, event: WindowEvent, cursor: [f32; 2]) -> NativeHostInput {
    NativeHostInput {
        ui_scale: 1.,
        context: Some(owner.clone()),
        cursor: Some(Vec2::from_array(cursor)),
        modifiers: default(),
        event,
        consumed: false,
        actions: vec![],
    }
}
fn button(state: ButtonState) -> WindowEvent {
    WindowEvent::MouseButtonInput(MouseButtonInput {
        button: MouseButton::Middle,
        state,
        window: Entity::PLACEHOLDER,
    })
}
fn moved(cursor: [f32; 2]) -> WindowEvent {
    WindowEvent::CursorMoved(CursorMoved {
        window: Entity::PLACEHOLDER,
        position: Vec2::from_array(cursor),
        delta: None,
    })
}

#[test]
fn ordered_paper_gestures_reuse_projection_clip_all_art_and_reject_retired_owners() {
    let (mut app, handle, _, _) = crate::native_viewport::interface_shell::tests::fixture();
    let owner = handle.frame().unwrap().context;
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let sheet: DrawingSheetDto = serde_json::from_value(
        json!({"id":1,"name":"Navigation","format":"a4","orientation":"landscape"}),
    )
    .unwrap();
    let source = edges::SourceKey::new(owner.clone(), 1, 1, &sheet);
    let mut state = Workbench {
        workspace: Workspace::Drawing,
        ..default()
    };
    state.refresh_owner(&owner);
    state.workspace = Workspace::Drawing;
    state.paper_key = Some((1, sheet, Default::default()));
    state.paper_view = Some(PaperView {
        camera,
        source: source.clone(),
        width: 1000.,
        height: 800.,
        side: 240.,
        marks: Vec::new(),
        art_context: Some((1, Default::default())),
        art_failure: None,
        navigation: Navigation::new(owner.clone(), 1, [297., 210.], pane(1000., 800., 240.))
            .unwrap(),
    });
    let mut cache = edges::EdgeCache::default();
    let key = raster(world, state.paper_view.as_ref().unwrap()).unwrap();
    let (image, region) = {
        let ready = cache
            .prepare(
                &mut world.resource_mut::<Assets<Image>>(),
                source,
                key,
                |_| panic!("Empty sheet must not project"),
            )
            .unwrap();
        (ready.image, ready.region)
    };
    world.insert_resource(cache);
    draw(world, &mut state, image.clone(), region).unwrap();
    let pane_entity = state.widgets.entity("drawing-content-clip").unwrap();
    let paper_entity = state.widgets.entity("drawing-paper").unwrap();
    assert_eq!(
        world.get::<Node>(pane_entity).unwrap().overflow,
        Overflow::clip()
    );
    assert_eq!(
        world.get::<Node>(paper_entity).unwrap().overflow,
        Overflow::clip()
    );
    assert_eq!(
        world.get::<ChildOf>(paper_entity).unwrap().parent(),
        pane_entity
    );
    let cursor = [540., 330.];
    let before = state.paper_view.as_ref().unwrap().navigation.transform();
    let picked = before.pick(cursor.map(f64::from)).unwrap();
    world.insert_resource(state);
    let mut wheel = input(
        &owner,
        WindowEvent::MouseWheel(MouseWheel {
            unit: MouseScrollUnit::Line,
            phase: bevy::input::touch::TouchPhase::Moved,
            x: 0.,
            y: 5.,
            window: Entity::PLACEHOLDER,
        }),
        cursor,
    );
    wheel.modifiers.ctrl = true;
    assert!(drawing_navigation_input::navigate(world, &handle, &wheel).unwrap());
    let after = world
        .resource::<Workbench>()
        .paper_view
        .as_ref()
        .unwrap()
        .navigation
        .transform();
    assert!(after.scale > before.scale);
    let actual = after.pick(cursor.map(f64::from)).unwrap();
    for i in 0..2 {
        assert!(
            (picked[i] - actual[i]).abs() < 1e-8,
            "Paper pick moved during anchored zoom"
        );
    }
    assert_eq!(
        world.resource::<Assets<Image>>().len(),
        1,
        "Navigation leaked a raster asset"
    );
    let state = world.resource::<Workbench>();
    let edge_entity = state.widgets.entity("drawing-projected-edges").unwrap();
    assert_eq!(world.get::<ImageNode>(edge_entity).unwrap().image, image);
    assert_eq!(
        world.get::<ChildOf>(edge_entity).unwrap().parent(),
        paper_entity
    );

    assert!(drawing_navigation_input::navigate(
        world,
        &handle,
        &input(&owner, button(ButtonState::Pressed), cursor)
    )
    .unwrap());
    let outside = [100., 80.];
    assert!(
        drawing_navigation_input::navigate(world, &handle, &input(&owner, moved(outside), outside))
            .unwrap(),
        "Captured pan must continue outside the pane"
    );
    assert_ne!(
        world
            .resource::<Workbench>()
            .paper_view
            .as_ref()
            .unwrap()
            .navigation
            .transform()
            .origin,
        after.origin
    );
    assert!(drawing_navigation_input::navigate(
        world,
        &handle,
        &input(&owner, button(ButtonState::Released), outside)
    )
    .unwrap());
    assert!(!drawing_navigation_input::navigate(
        world,
        &handle,
        &input(&owner, moved(cursor), cursor)
    )
    .unwrap());

    assert!(drawing_navigation_input::navigate(
        world,
        &handle,
        &input(&owner, button(ButtonState::Pressed), cursor)
    )
    .unwrap());
    let retained = world
        .resource::<Workbench>()
        .paper_view
        .as_ref()
        .unwrap()
        .navigation
        .transform();
    let mut retired = owner.clone();
    retired.epoch += 1;
    assert!(!drawing_navigation_input::navigate(
        world,
        &handle,
        &input(&retired, moved(outside), outside)
    )
    .unwrap());
    let state = world.resource::<Workbench>();
    assert!(!state.paper_view.as_ref().unwrap().navigation.is_panning());
    assert_eq!(
        state
            .paper_view
            .as_ref()
            .unwrap()
            .navigation
            .transform()
            .origin,
        retained.origin
    );
    assert!(!drawing_navigation_input::navigate(
        world,
        &handle,
        &input(&owner, moved(outside), outside)
    )
    .unwrap());
}

fn paper_fixture(world: &mut World, owner: &DocumentContext, sheet: &DrawingSheetDto) -> Workbench {
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let source = edges::SourceKey::new(owner.clone(), 1, 1, sheet);
    let mut state = Workbench {
        workspace: Workspace::Drawing,
        ..default()
    };
    state.refresh_owner(owner);
    state.workspace = Workspace::Drawing;
    state.paper_view = Some(PaperView {
        camera,
        source: source.clone(),
        width: 1000.,
        height: 800.,
        side: 240.,
        marks: Vec::new(),
        art_context: Some((1, Default::default())),
        art_failure: None,
        navigation: Navigation::new(
            owner.clone(),
            sheet.id,
            [297., 210.],
            pane(1000., 800., 240.),
        )
        .unwrap(),
    });
    let mut cache = edges::EdgeCache::default();
    let key = raster(world, state.paper_view.as_ref().unwrap()).unwrap();
    cache
        .prepare(
            &mut world.resource_mut::<Assets<Image>>(),
            source,
            key,
            |_| panic!("Empty sheet must not project"),
        )
        .unwrap();
    world.insert_resource(cache);
    state
}
fn note_sheet() -> DrawingSheetDto {
    serde_json::from_value(
        json!({"id":1,"name":"Bounded paper","format":"a4","orientation":"landscape",
        "annotations":[{"kind":"note","id":1,"text":"Saved note","position":[20.,20.]}]}),
    )
    .unwrap()
}

#[test]
fn actual_ui_stack_keeps_retained_paper_above_new_and_recreated_backdrops() {
    use bevy::ui::{ComputedStackIndex, UiPlugin, UiStack};

    let (mut app, handle, _, _) = crate::native_viewport::interface_shell::tests::fixture();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
        UiPlugin,
    ))
    .init_resource::<Assets<Image>>()
    .init_resource::<Assets<bevy::image::TextureAtlasLayout>>();
    let owner = handle.frame().unwrap().context;
    let sheet = note_sheet();
    let world = app.world_mut();
    let mut state = paper_fixture(world, &owner, &sheet);
    super::super::annotation_preview(world, &mut state, &sheet).unwrap();
    let camera = state.paper_view.as_ref().unwrap().camera;
    let clip = state.widgets.entity("drawing-content-clip").unwrap();
    let paper = state.widgets.entity("drawing-paper").unwrap();
    let raster = state.widgets.entity("drawing-projected-edges").unwrap();
    world.run_schedule(PostUpdate);
    let mut retired = None;
    for _ in 0..2 {
        paint_backdrop(world, camera, &mut state, 1000., 800., 240.);
        let backdrop = state.widgets.entity("drawing-backdrop").unwrap();
        assert_ne!(Some(backdrop), retired);
        world.run_schedule(PostUpdate);
        let stack = world.resource::<UiStack>();
        let background_index = world.get::<ComputedStackIndex>(backdrop).unwrap().0 as usize;
        assert_eq!(stack.uinodes[background_index], backdrop);
        let paper_partition = stack
            .partition
            .iter()
            .find(|range| stack.uinodes[range.start] == clip)
            .expect("The clip remains an independent stack root");
        assert!(
            background_index < paper_partition.start,
            "An opaque backdrop covered the retained paper subtree: {stack:?}"
        );
        let descendants = &stack.uinodes[paper_partition.clone()];
        assert!(descendants.contains(&paper) && descendants.contains(&raster));
        assert!(
            descendants.len() > 10,
            "Frame and annotation art were omitted"
        );
        let captured = diagnostics::snapshot(world, &state).unwrap();
        let row = captured["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["key"] == "drawing-backdrop")
            .unwrap();
        assert_eq!(row["stack_index"], background_index);

        state.widgets.begin();
        super::super::annotation_preview(world, &mut state, &sheet).unwrap();
        state.widgets.finish(world);
        assert!(world.get_entity(backdrop).is_err());
        assert_eq!(state.widgets.entity("drawing-paper"), Some(paper));
        assert_eq!(
            state.widgets.entity("drawing-projected-edges"),
            Some(raster)
        );
        world.run_schedule(PostUpdate);
        retired = Some(backdrop);
    }
}

#[test]
fn failed_annotation_preview_is_atomic_survives_navigation_and_cancel_recovers_retained_paper() {
    let (mut app, handle, _, _) = crate::native_viewport::interface_shell::tests::fixture();
    let owner = handle.frame().unwrap().context;
    let world = app.world_mut();
    let original = note_sheet();
    let saved = original.clone();
    let mut state = paper_fixture(world, &owner, &original);
    super::super::annotation_preview(world, &mut state, &original).unwrap();
    let paper = state.widgets.entity("drawing-paper").unwrap();
    assert_eq!(state.paper_labels[0].text, "Saved note");
    assert_eq!(state.paper_view.as_ref().unwrap().marks.len(), 1);
    let mut invalid = original.clone();
    if let limo_cad_sketch::DrawingAnnotationDto::Note { position, .. } =
        &mut invalid.annotations[0]
    {
        position[0] = 1e100;
    }
    let error = super::super::annotation_preview(world, &mut state, &invalid).unwrap_err();
    assert!(error.contains("finite render coordinates"), "{error}");
    assert!(state.paper_key.is_none());
    assert!(
        state.paper.is_empty() && state.paper_labels.is_empty() && state.paper_fills.is_empty()
    );
    assert!(state.paper_view.as_ref().unwrap().marks.is_empty());
    assert_eq!(world.get::<Node>(paper).unwrap().display, Display::None);
    let diagnostic = state.widgets.entity("drawing-render-error").unwrap();
    assert_eq!(world.get::<Text>(diagnostic).unwrap().0, error);
    for _ in 0..2 {
        assert_eq!(repaint(world, &mut state).unwrap_err(), error);
    }
    assert_eq!(world.get::<Text>(diagnostic).unwrap().0, error);
    assert_eq!(world.resource::<Assets<Image>>().len(), 1);
    super::super::annotation_preview(world, &mut state, &original).unwrap();
    assert!(state.paper_key.is_some());
    assert_eq!(state.paper_labels[0].text, "Saved note");
    assert_eq!(state.paper_view.as_ref().unwrap().marks.len(), 1);
    assert_ne!(world.get::<Node>(paper).unwrap().display, Display::None);
    assert_eq!(
        world.get::<Node>(diagnostic).unwrap().display,
        Display::None
    );
    assert_eq!(world.resource::<Assets<Image>>().len(), 1);
    assert_eq!(original, saved, "Presentation mutated saved drawing intent");
}
#[test]
fn annotation_failures_cache_exact_sheet_units_and_projection_owner_only() {
    let (mut app, handle, _, _) = crate::native_viewport::interface_shell::tests::fixture();
    let owner = handle.frame().unwrap().context;
    let sheet = note_sheet();
    let mut state = paper_fixture(app.world_mut(), &owner, &sheet);
    let view = state.paper_view.as_mut().unwrap();
    let units = limo_cad_core::UnitSystem::Mm;
    assert_eq!(
        view.annotations_with(&sheet, units, || Err("bounded work".into()))
            .err()
            .unwrap(),
        "bounded work"
    );
    assert_eq!(
        view.annotations_with(&sheet, units, || panic!(
            "Cached failure regenerated graphics"
        ))
        .err()
        .unwrap(),
        "bounded work"
    );
    let mut corrected = sheet.clone();
    corrected.annotations.clear();
    view.annotations_with(&corrected, units, || Ok(annotations::Art::default()))
        .unwrap();
    assert!(view.art_failure.is_none());
    view.annotations_with(&sheet, units, || Err("old owner".into()))
        .err()
        .unwrap();
    view.annotations_with(&sheet, limo_cad_core::UnitSystem::In, || {
        Ok(annotations::Art::default())
    })
    .unwrap();
    assert!(
        view.art_failure.is_none(),
        "Unit change reused stale failure"
    );
    view.annotations_with(&sheet, units, || Err("old revision".into()))
        .err()
        .unwrap();
    view.source = edges::SourceKey::new(owner.clone(), 2, 1, &sheet);
    view.annotations_with(&sheet, units, || Ok(annotations::Art::default()))
        .unwrap();
    assert!(
        view.art_failure.is_none(),
        "Document revision change reused stale failure"
    );
    view.annotations_with(&sheet, units, || Err("old owner".into()))
        .err()
        .unwrap();
    let mut next_owner = owner;
    next_owner.epoch += 1;
    view.source = edges::SourceKey::new(next_owner, 1, 1, &sheet);
    view.annotations_with(&sheet, units, || Ok(annotations::Art::default()))
        .unwrap();
    assert!(
        view.art_failure.is_none(),
        "New owner reused retired failure"
    );
}

#[test]
fn annotation_preview_reuses_frame_artwork_and_title_edits_refresh_it() {
    let (mut app, handle, _, _) = crate::native_viewport::interface_shell::tests::fixture();
    let owner = handle.frame().unwrap().context;
    let world = app.world_mut();
    let original = note_sheet();
    let mut state = paper_fixture(world, &owner, &original);
    super::super::annotation_preview(world, &mut state, &original).unwrap();
    let segments = world
        .resource::<FrameCache>()
        .0
        .as_ref()
        .unwrap()
        .1
        .segments
        .as_ptr();

    let mut preview = original.clone();
    let limo_cad_sketch::DrawingAnnotationDto::Note { text, .. } = &mut preview.annotations[0]
    else {
        panic!("Note fixture required")
    };
    *text = "Changed note".into();
    super::super::annotation_preview(world, &mut state, &preview).unwrap();
    assert_eq!(state.paper_labels[0].text, "Changed note");
    let cache = world.resource::<FrameCache>().0.as_ref().unwrap();
    assert_eq!(cache.1.segments.as_ptr(), segments);

    preview.title_block.title = "Changed title".into();
    super::super::annotation_preview(world, &mut state, &preview).unwrap();
    let cache = world.resource::<FrameCache>().0.as_ref().unwrap();
    let art = &cache.1;
    assert_ne!(art.segments.as_ptr(), segments);
    assert!(art.labels.iter().any(|label| label.text == "Changed title"));
}

#[test]
fn frame_failure_is_atomic_cached_across_navigation_and_corrected_style_recovers() {
    let (mut app, handle, _, _) = crate::native_viewport::interface_shell::tests::fixture();
    let owner = handle.frame().unwrap().context;
    let world = app.world_mut();
    let original = note_sheet();
    let mut state = paper_fixture(world, &owner, &original);
    super::super::annotation_preview(world, &mut state, &original).unwrap();
    let paper = state.widgets.entity("drawing-paper").unwrap();
    let mut invalid = original.clone();
    invalid.style.visible.dash_mm = vec![1e-100, 1e-100];
    let error = super::super::annotation_preview(world, &mut state, &invalid).unwrap_err();
    assert!(error.starts_with("Drawing frame:"), "{error}");
    assert!(
        state.paper_key.is_none()
            && state.paper.is_empty()
            && state.paper_labels.is_empty()
            && state.paper_fills.is_empty()
    );
    assert!(state.paper_view.as_ref().unwrap().marks.is_empty());
    assert_eq!(world.get::<Node>(paper).unwrap().display, Display::None);
    let diagnostic = state.widgets.entity("drawing-render-error").unwrap();
    assert_eq!(world.get::<Text>(diagnostic).unwrap().0, error);
    let cache = world.resource::<FrameCache>().0.as_ref().unwrap();
    assert!(cache.0 == frame::Source::new(&original, [297., 210.]));
    let failed = state
        .paper_view
        .as_ref()
        .unwrap()
        .art_failure
        .as_ref()
        .unwrap();
    assert_eq!(failed.sheet, invalid);
    assert_eq!(failed.error, error);
    for _ in 0..2 {
        assert_eq!(repaint(world, &mut state).unwrap_err(), error);
    }
    assert_eq!(world.get::<Text>(diagnostic).unwrap().0, error);
    super::super::annotation_preview(world, &mut state, &original).unwrap();
    assert!(state.paper_key.is_some());
    assert_eq!(state.paper_labels[0].text, "Saved note");
    assert_eq!(state.paper_view.as_ref().unwrap().marks.len(), 1);
    assert_ne!(world.get::<Node>(paper).unwrap().display, Display::None);
    assert_eq!(
        world.get::<Node>(diagnostic).unwrap().display,
        Display::None
    );
    assert!(
        world.resource::<FrameCache>().0.as_ref().unwrap().0
            == frame::Source::new(&original, [297., 210.])
    );
    assert_eq!(world.resource::<Assets<Image>>().len(), 1);
}
