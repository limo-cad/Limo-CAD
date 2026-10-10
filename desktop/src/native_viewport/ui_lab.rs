//! Standalone, dev-only visual regression surface for native Bevy UI.
//!
//! It renders the exact production HUD builders into an offscreen GPU image,
//! captures that image, then exits. The resulting PNG records production
//! geometry, layout and themes for visual review.

use std::path::PathBuf;

use bevy::{
    app::SubApps,
    asset::RenderAssetUsages,
    camera::RenderTarget,
    image::Image,
    prelude::*,
    render::{
        render_resource::{Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages},
        renderer::RenderDevice,
        view::screenshot::{Screenshot, ScreenshotCaptured},
        RenderPlugin,
    },
    window::{ExitCondition, WindowPlugin},
};

use super::{
    ui::{self, HudAxisLabel, HudAxisMark, NativeHudRoot, ViewportUiAssets, ViewportUiTheme},
    ViewportCamera, ViewportHud, ViewportHudRow, ViewportHudSelection,
};

#[derive(Resource)]
struct CaptureRequest {
    path: PathBuf,
}

#[derive(Resource, Default)]
struct CaptureComplete(bool);

#[derive(Resource, Clone)]
struct LabTarget(Handle<Image>);

#[derive(Resource, Clone, Copy)]
struct LabCamera(ViewportCamera);

#[derive(Resource, Clone, Copy)]
struct LabPalette(super::ViewportPalette);

#[derive(Resource)]
struct CamLabMesh(limo_cad_cam::CamSimulationMeshDto);

pub fn run(output: PathBuf) {
    let palette = ui::light_reference_palette();
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(
        palette.background[0],
        palette.background[1],
        palette.background[2],
    )))
    .insert_resource(CaptureRequest { path: output })
    .init_resource::<CaptureComplete>()
    .insert_resource(LabCamera(ViewportCamera::default()))
    .insert_resource(LabPalette(palette))
    .init_resource::<ViewportUiAssets>()
    .add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            .set(bevy::log::LogPlugin {
                filter: "info,wgpu_core=warn,wgpu_hal=warn".to_string(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            }),
    );
    if std::env::var_os("LIMO_CAD_FEATURE_LAB_MODEL").is_some() {
        super::interface_shell::install_visual_lab(&mut app);
        app.add_systems(Startup, (ui::load_system_font, setup_feature_lab).chain());
    } else if std::env::var_os("LIMO_CAD_RIBBON_LAB").is_some() {
        super::interface_shell::install_visual_lab(&mut app);
        app.add_systems(Startup, (ui::load_system_font, setup_ribbon_lab).chain());
    } else if let Some(path) = std::env::var_os("LIMO_CAD_CAM_LAB_MESH") {
        let mesh = serde_json::from_slice(&std::fs::read(path).expect("read CAM capture mesh"))
            .expect("parse CAM capture mesh");
        app.insert_resource(CamLabMesh(mesh))
            .add_systems(Startup, setup_cam_lab);
    } else {
        app.add_systems(Startup, (ui::load_system_font, setup_lab).chain())
            .add_systems(Update, update_lab_orientation);
    }

    app.finish();
    app.cleanup();
    let mut sub_apps = std::mem::take(app.sub_apps_mut());

    update_and_wait(&mut sub_apps);
    let target = sub_apps.main.world().resource::<LabTarget>().0.clone();
    sub_apps
        .main
        .world_mut()
        .spawn(Screenshot::image(target))
        .observe(save_capture);
    super::screenshot::until_captured(|| {
        update_and_wait(&mut sub_apps);
        Ok(sub_apps.main.world().resource::<CaptureComplete>().0)
    })
    .expect("Bevy UI lab screenshot did not complete");
}

/// GPU capture of the production feature panel against a saved workflow model.
/// This uses the typed form and actual widgets without creating an OS window.
fn setup_feature_lab(world: &mut World) {
    use crate::{
        native_forms::{FormModel, SolidForm, SolidFormKind},
        session_bridge::{native_interface::feature, parse_engine_envelope},
        state::AppState,
    };
    use limo_cad_interface::{DocumentContext, Rect};
    let path = std::env::var_os("LIMO_CAD_FEATURE_LAB_MODEL").expect("feature capture model");
    let engine = AppState::new();
    parse_engine_envelope(engine.project_load(&std::fs::read_to_string(path).expect("read model")))
        .expect("load workflow model");
    let kind = match std::env::var("LIMO_CAD_FEATURE_LAB").as_deref() {
        Ok("extrude") => SolidFormKind::Extrude,
        Ok("fillet") => SolidFormKind::Fillet,
        Ok("hole") => SolidFormKind::Hole,
        _ => panic!("LIMO_CAD_FEATURE_LAB must be extrude, fillet or hole"),
    };
    let document = engine.document_snapshot();
    let viewport = engine.viewport_frame();
    let owner = DocumentContext {
        window_id: "feature-lab".into(),
        document_id: "feature-lab".into(),
        epoch: 1,
    };
    let model = FormModel {
        owner: &owner,
        engine_revision: 1,
        document: &document,
        profiles: &viewport.document.profile_catalog,
        scene: &viewport.document.scene,
        datum_planes: &viewport.document.datum_planes,
        parameters: &[],
        assembly: None,
        assembly_solution: None,
    };
    let mut form = SolidForm::new_kind(kind, &model);
    if kind == SolidFormKind::Extrude {
        form.set_profiles(
            vec![limo_cad_solid::ProfileRefDto {
                sketch_name: "Sketch1".into(),
                profile_index: 0,
            }],
            &model,
        )
        .expect("select workflow profile");
    }
    let mut target = Image::new_uninit(
        Extent3d {
            width: 500,
            height: 860,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let target = world.resource_mut::<Assets<Image>>().add(target);
    world.insert_resource(LabTarget(target.clone()));
    let theme = ViewportUiTheme::from_palette(&super::ViewportPalette::default());
    world.insert_resource(ClearColor(theme.header));
    // Production control styling activates its interface camera only while a
    // document frame is presented, including this windowless capture surface.
    let bounds = Rect {
        x: 0.,
        y: 0.,
        width: 500.,
        height: 860.,
    };
    world
        .resource::<super::interface_shell::NativeInterfaceHandle>()
        .present(super::interface_shell::InterfaceFrame {
            context: owner.clone(),
            client: bounds,
            surface: bounds,
            canvases: Vec::new(),
            surfaces: Vec::new(),
            modal_stack: Vec::new(),
            document_visible: true,
        })
        .expect("present feature capture surface");
    world.spawn((
        Camera2d,
        super::interface_shell::InterfaceCamera,
        RenderTarget::Image(target.into()),
        IsDefaultUiCamera,
        BoxShadowSamples(6),
    ));
    feature::panel::synchronize_snapshot(
        world,
        &owner,
        Rect {
            x: 110.,
            y: 32.,
            width: 320.,
            height: 680.,
        },
        Some(feature::FeaturePanel {
            title: if kind == SolidFormKind::Fillet {
                "Solid Fillet".into()
            } else {
                kind.label().into()
            },
            kind,
            form_id: 1,
            fields: form.fields(&model),
            can_apply: form.can_apply(&model),
            busy: false,
            error: None,
            preview_notice: None,
            notes: form.feature_notes(),
            pick_target: None,
            choice_field: None,
            presentation: form.presentation(&model),
        }),
    )
    .expect("render production feature panel");
}

/// Lossless GPU readback of the production ribbon widgets, avoiding desktop
/// capture compression when comparing typography and one-pixel strokes.
fn setup_ribbon_lab(world: &mut World) {
    use super::interface_shell::{
        self,
        ribbon::{self, Icon},
        InterfaceControl,
    };
    let mut target = Image::new_uninit(
        Extent3d {
            width: 1360,
            height: 280,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let target = world.resource_mut::<Assets<Image>>().add(target);
    world.insert_resource(LabTarget(target.clone()));
    let theme = ViewportUiTheme::from_palette(&super::ViewportPalette::default());
    world.insert_resource(ClearColor(theme.header.with_alpha(1.)));
    let camera = world
        .spawn((
            Camera2d,
            RenderTarget::Image(target.into()),
            IsDefaultUiCamera,
        ))
        .id();
    let assets = world.resource::<ViewportUiAssets>().clone();
    for row in 0..3 {
        for (index, (label, icon)) in [
            ("Line", Icon::Line),
            ("Three-point arc", Icon::Arc),
            ("Rectangle", Icon::Rectangle),
            ("Circle", Icon::Circle),
            ("Fit-point spline", Icon::Spline),
            ("Center-to-center slot", Icon::Slot),
            ("Create Sketch", Icon::Sketch),
            ("Extrude", Icon::Extrude),
        ]
        .into_iter()
        .enumerate()
        {
            let mut control = InterfaceControl::button("ribbon-lab", label);
            control.disabled = row == 2;
            control.selected = Some(row == 1);
            let entity = interface_shell::spawn_button(
                &mut world.commands(),
                camera,
                ribbon::node(60. + index as f32 * 50., 34. + row as f32 * 80., 48.),
                control,
                theme,
                &assets,
            );
            world.flush();
            ribbon::decorate(world, entity, icon);
        }
    }
    let entity = interface_shell::spawn_button(
        &mut world.commands(),
        camera,
        ribbon::finish_node(8., 57.5, true),
        InterfaceControl::button("ribbon-lab", "Finish sketch"),
        theme,
        &assets,
    );
    world.flush();
    ribbon::decorate(world, entity, Icon::Finish);
}

/// Uses the production stock material, lights and AA, not browser screenshot
/// rendering. The mesh is produced by the opt-in CAM kernel capture test.
fn setup_cam_lab(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    source: Res<CamLabMesh>,
) {
    let mut target = Image::new_uninit(
        Extent3d {
            width: 1440,
            height: 900,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let target = images.add(target);
    commands.insert_resource(LabTarget(target.clone()));
    let positions: Vec<[f32; 3]> = source
        .0
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    let min = positions.iter().fold(Vec3::splat(f32::INFINITY), |a, p| {
        a.min(Vec3::from_array(*p))
    });
    let max = positions
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |a, p| {
            a.max(Vec3::from_array(*p))
        });
    let closeup = std::env::var_os("LIMO_CAD_CAM_LAB_CLOSEUP").is_some();
    let center = if closeup {
        Vec3::new(22.0, 25.0, -5.0)
    } else {
        (min + max) * 0.5
    };
    let extent = if closeup {
        18.0
    } else {
        (max - min).max_element()
    };
    let direction = if closeup {
        Vec3::new(0.15, -0.3, 1.3)
    } else {
        Vec3::new(0.15, -0.8, 1.15)
    };
    let eye = center + direction.normalize() * extent * 2.8;
    let view = ViewportCamera {
        position: eye.to_array(),
        target: center.to_array(),
        up: Vec3::Z.to_array(),
        vertical_fov_degrees: 24.0,
    };
    commands.spawn((
        Camera3d::default(),
        RenderTarget::Image(target.into()),
        super::platform::VIEWPORT_MSAA,
        Projection::Perspective(PerspectiveProjection {
            fov: view.vertical_fov_degrees.to_radians(),
            ..default()
        }),
        Transform::from_translation(eye).looking_at(center, Vec3::Z),
    ));
    commands.insert_resource(GlobalAmbientLight {
        brightness: 500.0,
        ..default()
    });
    let (key, fill) = super::platform::cam_light_transforms(view);
    for (transform, illuminance) in [(key, 2600.0), (fill, 750.0)] {
        commands.spawn((
            DirectionalLight {
                illuminance,
                shadow_maps_enabled: false,
                ..default()
            },
            transform,
        ));
    }
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_NORMAL,
        source
            .0
            .normals
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect::<Vec<_>>(),
    );
    commands.spawn((
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(materials.add(super::platform::cam_stock_material())),
    ));
}

fn update_and_wait(sub_apps: &mut SubApps) {
    sub_apps.update();
    sub_apps
        .main
        .world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("Bevy UI lab GPU wait failed");
}

fn setup_lab(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    assets: Res<ViewportUiAssets>,
    palette: Res<LabPalette>,
    native_locale: Option<Res<crate::native_viewport::localization::NativeLocale>>,
) {
    let mut target = Image::new_uninit(
        Extent3d {
            width: 1440,
            height: 900,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    target.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    let target = images.add(target);
    commands.insert_resource(LabTarget(target.clone()));
    let render_target: RenderTarget = target.into();

    let camera = commands
        .spawn((
            Name::new("Bevy UI lab camera"),
            Camera3d::default(),
            render_target,
            IsDefaultUiCamera,
            BoxShadowSamples(6),
            Transform::from_xyz(0.0, -8.0, 6.0).looking_at(Vec3::ZERO, Vec3::Z),
        ))
        .id();
    let theme = ViewportUiTheme::from_palette(&palette.0);

    spawn_reference_grid(&mut commands, camera, theme);

    let locale = crate::native_viewport::localization::locale_of(native_locale.as_deref());
    let t = |key| crate::app_preferences::locale::translate(locale, key);
    let hud = ViewportHud {
        render_native_chrome: true,
        nav_tool: "orbit".to_string(),
        sketch_mode: true,
        can_undo: true,
        can_redo: false,
        six_dof_state: "connected".to_string(),
        hovered_control: "nav:pan".to_string(),
        pressed_control: String::new(),
        prompt: Some(t("sketch.pickPlanePrompt").to_string()),
        dof_label: Some("DOF 4".to_string()),
        coordinate_readout: None,
        dim_opacity: 0.20,
        selection: Some(ViewportHudSelection {
            title: t("selectionReadout.title").to_string(),
            subject: "Body1".to_string(),
            rows: vec![
                ViewportHudRow {
                    label: t("selectionReadout.measurements.size").to_string(),
                    value: "30 × 30 × 30 mm".to_string(),
                },
                ViewportHudRow {
                    label: t("selectionReadout.measurements.surfaceArea").to_string(),
                    value: "≈ 5,400 mm²".to_string(),
                },
                ViewportHudRow {
                    label: t("selectionReadout.measurements.volume").to_string(),
                    value: "≈ 27,000 mm³".to_string(),
                },
            ],
            footer: Some(t("selectionReadout.approximate").to_string()),
        }),
    };
    ui::spawn_viewport_hud(&mut commands, camera, &hud, &palette.0, &assets, locale);
    ui::spawn_reference_dialog(&mut commands, camera, theme, &assets, locale);
}

fn spawn_reference_grid(commands: &mut Commands, camera: Entity, theme: ViewportUiTheme) {
    commands
        .spawn((
            Name::new("Bevy UI lab viewport reference"),
            UiTargetCamera(camera),
            NativeHudRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                top: px(0.0),
                width: percent(100.0),
                height: percent(100.0),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.viewport),
            ZIndex(-100),
        ))
        .with_children(|grid| {
            for index in 0..=24 {
                let major = index % 5 == 0;
                grid.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(index as f32 * 100.0 / 24.0),
                        top: px(0.0),
                        width: px(if major { 1.2 } else { 0.7 }),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(theme.edge.with_alpha(if major { 0.42 } else { 0.20 })),
                ));
            }
            for index in 0..=15 {
                let major = index % 5 == 0;
                grid.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0.0),
                        top: percent(index as f32 * 100.0 / 15.0),
                        width: percent(100.0),
                        height: px(if major { 1.2 } else { 0.7 }),
                        ..default()
                    },
                    BackgroundColor(theme.edge.with_alpha(if major { 0.42 } else { 0.20 })),
                ));
            }
        });
}

fn update_lab_orientation(
    camera: Res<LabCamera>,
    mut marks: Query<(&HudAxisMark, &mut Node)>,
    mut labels: Query<(&HudAxisLabel, &mut Node), Without<HudAxisMark>>,
) {
    ui::update_orientation_nodes(camera.0, &mut marks, &mut labels);
}

fn save_capture(
    capture: On<ScreenshotCaptured>,
    request: Res<CaptureRequest>,
    mut complete: ResMut<CaptureComplete>,
) {
    let sample = capture
        .image
        .data
        .as_deref()
        .map(|bytes| &bytes[..bytes.len().min(16)]);
    eprintln!(
        "Captured {:?} {:?}; first bytes: {sample:?}",
        capture.image.texture_descriptor.size, capture.image.texture_descriptor.format
    );
    let bytes = super::screenshot::png_bytes(&capture.image).expect("encode Bevy capture");
    std::fs::write(&request.path, bytes).expect("save Bevy capture");
    complete.0 = true;
}
