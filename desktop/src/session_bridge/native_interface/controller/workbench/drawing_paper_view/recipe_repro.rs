//! Opt-in reproduction through authored commands and the production Bevy paper path.
use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};

#[test]
#[ignore = "Runs the complete turbine recipe and native projection; invoke explicitly for drawing changes"]
fn turbine_assembly_sheet_binds_placed_linework_to_the_bevy_paper_image() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let catalog = limo_cad_mcp::script_examples();
    let recipe = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "vertical-axis-turbine")
        .unwrap();
    let replay =
        limo_cad_mcp::run_script(recipe["source"].as_str().unwrap(), None, None, "fast", 1.)
            .unwrap();
    let model = &replay["exports"]["final_model"];
    assert!(model.is_object());
    if let Some(directory) = std::env::var_os("LIMO_CAD_DRAWING_REPRO_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("turbine-model.json"),
            serde_json::to_vec_pretty(model).unwrap(),
        )
        .unwrap();
    }
    let fixture = Fixture::new();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cad_load_project_model",
            &json!({"model_json":serde_json::to_string(model).unwrap()}),
            || Ok(()),
        )
        .unwrap();
    let drawing = fixture.engine.drawing_snapshot();
    let sheet = drawing
        .sheets
        .iter()
        .find(|s| s.title_block.drawing_number == "TUR-000")
        .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "drawing_select_sheet",
            &json!({"sheet_id":sheet.id}),
            || Ok(()),
        )
        .unwrap();
    let (mut app, _, _, _) = crate::native_viewport::interface_shell::tests::fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn(InterfaceCamera).id();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut state = Workbench::default();
    state.refresh_owner(&fixture.owner());
    state.workspace = Workspace::Drawing;
    paint(
        world,
        camera,
        &services,
        &mut state,
        (1280., 900., 240.),
        &HashMap::new(),
    )
    .unwrap();
    assert!(
        state.paper_key.is_some(),
        "Paper failed: {:?}",
        state
            .paper_view
            .as_ref()
            .and_then(|v| v.art_failure.as_ref().map(|f| &f.error))
    );
    let view = state.paper_view.as_ref().unwrap();
    let projections = world
        .resource::<edges::EdgeCache>()
        .projections(&view.source)
        .unwrap();
    assert_eq!(projections.len(), 2);
    for (_, projection) in projections.values() {
        assert!(
            !projection.visible.is_empty(),
            "Placed assembly view contains no visible lines"
        );
        assert!(
            projection
                .visible
                .iter()
                .map(|line| line.points.len())
                .sum::<usize>()
                > 100
        );
    }
    let entity = state.widgets.entity("drawing-projected-edges").unwrap();
    let paper = state.widgets.entity("drawing-paper").unwrap();
    let image = world.get::<ImageNode>(entity).unwrap().image.clone();
    assert_eq!(world.get::<ChildOf>(entity).unwrap().parent(), paper);
    let rendered = world.resource::<Assets<Image>>().get(&image).unwrap();
    let pixels = rendered.data.as_ref().unwrap();
    assert!(
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] != 0)
            .count()
            > 1000,
        "Crosshairs alone cannot satisfy the linework proof"
    );
    if let Some(directory) = std::env::var_os("LIMO_CAD_DRAWING_REPRO_DIR") {
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let size = resvg::tiny_skia::IntSize::from_wh(
            rendered.texture_descriptor.size.width,
            rendered.texture_descriptor.size.height,
        )
        .unwrap();
        let linework = resvg::tiny_skia::Pixmap::from_vec(pixels.clone(), size).unwrap();
        let mut proof = resvg::tiny_skia::Pixmap::new(size.width(), size.height()).unwrap();
        proof.fill(resvg::tiny_skia::Color::WHITE);
        proof.draw_pixmap(
            0,
            0,
            linework.as_ref(),
            &resvg::tiny_skia::PixmapPaint::default(),
            resvg::tiny_skia::Transform::identity(),
            None,
        );
        proof
            .save_png(directory.join("turbine-bevy-bound-linework.png"))
            .unwrap();
        let exported = parse_engine_envelope(
            fixture
                .engine
                .drawing_export(&json!({"sheet_id":sheet.id,"format":"svg"}).to_string()),
        )
        .unwrap();
        std::fs::write(
            directory.join("turbine-assembly.svg"),
            exported["content"].as_str().unwrap(),
        )
        .unwrap();
    }
    let document = state.paper_document.as_ref().unwrap().1.clone();
    let assets = world.resource::<Assets<Image>>().len();
    paint(
        world,
        camera,
        &services,
        &mut state,
        (1280., 900., 240.),
        &HashMap::new(),
    )
    .unwrap();
    assert!(Arc::ptr_eq(
        &document,
        &state.paper_document.as_ref().unwrap().1
    ));
    assert_eq!(world.resource::<Assets<Image>>().len(), assets);
    assert_eq!(world.get::<ImageNode>(entity).unwrap().image, image);
}
