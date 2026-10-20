//! The existing GPU/UI CI suite also exercises the real detached package entry.
//! No live window, external session or live document is supplied to the job.
use super::*;
use crate::native_viewport::interface_shell::{InterfaceCamera, InterfaceFrame};
use limo_cad_interface::{ControlKey, DocumentContext, KeyChord, Rect, Surface};
use limo_cad_project_file::{ProjectArchive, ReplayPackage, SaveMetadata};

fn source() -> String {
    let call = |id: &str, operation: &str, arguments: Value| {
        json!({"id":id,"call":{
            "group":limo_cad_interface::catalog::group_for(operation).unwrap(),
            "operation":operation,"arguments":arguments
        }})
    };
    json!({"version":2,"name":"Detached assembly preview","starting_state":"project","steps":[
        call("begin","sketch_begin",json!({"name":"Preview stock","plane":{"type":"origin_plane","plane":"xy"}})),
        call("rectangle","sketch_add_rectangle_locked",json!({"mode":"two_point",
            "anchor":{"x":0,"y":0},"corner_hint":{"x":40,"y":20},
            "width_mm":40,"height_mm":20,"ctrl_held":true})),
        call("finish","sketch_finish",json!({})),
        call("extrude","solid_extrude",json!({"sketch_name":"Preview stock","profile_indices":[0],
            "operation":"new_body","extent":{"type":"distance","distance":8},
            "taper_angle_deg":0,"flip":false,"target_body_ids":[]})),
        call("scene","solid_scene",json!({})),
        call("component","assembly_create_component",json!({"name":"Reusable stock",
            "body_ids":[{"$ref":"scene","pointer":"/bodies/0/id"}],"absorb_promoted_bodies":true})),
        call("occurrence","assembly_create_occurrence",json!({"name":"Rotated copy",
            "component_id":{"$ref":"component","pointer":"/id"},
            "local_pose":{"translation":[120,0,0],
                "rotation":[0,0,std::f64::consts::FRAC_1_SQRT_2,std::f64::consts::FRAC_1_SQRT_2]}}))
    ]}).to_string()
}

fn entry(app: &mut App, source: &str, archive: &[u8]) -> Arc<ReplayPackage> {
    let package = Arc::new(ReplayPackage::new(source.to_owned(), archive.to_vec()).unwrap());
    let identity = package.source_identity(source).unwrap();
    let artifact = Artifact::Project {
        package: package.clone(),
        opened: identity.clone(),
        saved: identity,
    };
    let loaded = artifact.inspect(None, source).unwrap();
    let files = &mut app.world_mut().resource_mut::<Files>();
    files.scripts = true;
    files.script.accept(loaded).unwrap();
    files.script.editor_open = true;
    package
}

fn text_fits(app: &App, widgets: &chrome::Widgets, key: &str) {
    let entity = widgets.entity(key).unwrap();
    let node = app.world().get::<ComputedNode>(entity).unwrap();
    let text = app
        .world()
        .get::<bevy::text::TextLayoutInfo>(entity)
        .unwrap();
    assert!(!text.glyphs.is_empty(), "{key} must shape native text");
    assert!(
        text.size.x <= node.size.x + 1. && text.size.y <= node.size.y + 1.,
        "{key} clipped: {:?} > {:?}",
        text.size,
        node.size
    );
}

fn button_bottom(app: &App, widgets: &chrome::Widgets) -> f32 {
    let entity = widgets.entity("scripts-preview-next").unwrap();
    let node = app.world().get::<ComputedNode>(entity).unwrap();
    let center = app
        .world()
        .get::<bevy::ui::UiGlobalTransform>(entity)
        .unwrap()
        .translation;
    center.y + node.size.y / 2.
}

pub(super) fn capture(out: &Path) {
    assert!(
        !out.exists(),
        "Retain prior package evidence; use a fresh proof path"
    );
    std::fs::create_dir_all(out).unwrap();
    let mut app = App::new();
    app.add_plugins(
        crate::native_viewport::script_preview::shared_fixture_plugins()
            .set(WindowPlugin {
                primary_window: Some(Window::default()),
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<PipelinedRenderingPlugin>(),
    );
    let handle = NativeInterfaceHandle::new(|| {});
    interface_shell::install(&mut app, handle.clone(), |_, _| {});
    crate::native_viewport::script_preview::install_shared_fixture(&mut app);
    app.init_resource::<Capture>();
    app.finish();
    app.cleanup();
    initialize(
        app.world_mut(),
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    let theme = crate::native_viewport::ui::theme(app.world());
    app.insert_resource(ClearColor(theme.viewport));
    app.insert_resource(bevy::ui::UiScale(1.75));
    update(&mut app);
    let mut texture = Image::new_fill(
        Extent3d {
            width: 1200,
            height: 760,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    texture.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(texture);
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<InterfaceCamera>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(camera)
        .insert(RenderTarget::Image(target.clone().into()));
    let bounds = Rect {
        x: 0.,
        y: 0.,
        width: 1200. / 1.75,
        height: 760. / 1.75,
    };
    let owner = DocumentContext {
        window_id: "package-evidence".into(),
        document_id: "live-fixture".into(),
        epoch: 1,
    };
    handle
        .present(InterfaceFrame {
            context: owner.clone(),
            client: bounds,
            surface: bounds,
            canvases: vec![],
            surfaces: vec![Surface {
                name: "document/scripts".into(),
                text: None,
            }],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    let before_view = crate::native_viewport::interface_view_snapshot(app.world());
    let before_revision = crate::native_viewport::interface_model_revision(app.world());
    let archive = ProjectArchive::new(
        limo_cad_sketch::SketchManager::new()
            .export_project_model()
            .unwrap(),
        SaveMetadata {
            application_version: "native-preview-test",
            saved_at: "2026-10-20T00:00:00Z",
        },
    )
    .unwrap()
    .encode()
    .unwrap();
    let source = source();
    let mut widgets = chrome::Widgets::default();
    for locale in crate::app_preferences::locale::SUPPORTED {
        crate::native_viewport::localization::set_locale(app.world_mut(), locale);
        let language = crate::app_preferences::locale::code(locale);
        let locale_out = out.join(language);
        std::fs::create_dir_all(&locale_out).unwrap();
        let package = entry(&mut app, &source, &archive);
        widgets.begin();
        editor::paint_source(
            app.world_mut(),
            camera,
            &mut widgets,
            bounds.width as f32,
            bounds.height as f32,
            theme,
            false,
        )
        .unwrap();
        widgets.finish(app.world_mut());
        for _ in 0..3 {
            update(&mut app);
        }
        let open = widgets.entity("scripts-preview-open").unwrap();
        assert!(!app.world().get::<InterfaceControl>(open).unwrap().disabled);
        let activation = handle.resolve_retained(ControlKey(open.to_bits())).unwrap();
        handle.prepare_activation(&activation).unwrap();
        assert_eq!(handle.focused_key(), Some(ControlKey(open.to_bits())));
        super::capture(&mut app, &target, &locale_out.join("entry.png"));
        command(
            app.world_mut(),
            &handle,
            Action::Open,
            &ControlInput::Key(KeyChord::plain("Enter")),
        )
        .unwrap();
        widgets.begin();
        panel::paint(
            app.world_mut(),
            camera,
            &mut widgets,
            bounds.width as f32,
            bounds.height as f32,
            theme,
        )
        .unwrap();
        widgets.finish(app.world_mut());
        settle(&mut app);
        let preview = &app.world().resource::<Files>().script.preview;
        assert!(preview.project);
        assert_eq!(preview.count(), 2);
        assert_eq!(preview.assembly_notices(), (0, true));
        widgets.begin();
        panel::paint(
            app.world_mut(),
            camera,
            &mut widgets,
            bounds.width as f32,
            bounds.height as f32,
            theme,
        )
        .unwrap();
        widgets.finish(app.world_mut());
        super::capture(&mut app, &target, &locale_out.join("baseline.png"));
        let baseline_bottom = button_bottom(&app, &widgets);
        assert!(
            baseline_bottom <= 760.,
            "Package navigation escaped the minimum window"
        );
        command(
            app.world_mut(),
            &handle,
            Action::Next,
            &ControlInput::Key(KeyChord::plain("Enter")),
        )
        .unwrap();
        settle(&mut app);
        widgets.begin();
        panel::paint(
            app.world_mut(),
            camera,
            &mut widgets,
            bounds.width as f32,
            bounds.height as f32,
            theme,
        )
        .unwrap();
        widgets.finish(app.world_mut());
        super::capture(&mut app, &target, &locale_out.join("final.png"));
        assert!(
            app.world()
                .resource::<Files>()
                .script
                .preview
                .assembly_notices()
                .0
                > 0
        );
        for key in [
            "scripts-preview-caption",
            "scripts-preview-scope",
            "scripts-preview-assembly-notices",
        ] {
            text_fits(&app, &widgets, key);
        }
        assert_eq!(
            button_bottom(&app, &widgets),
            baseline_bottom,
            "Assembly notices must not move the navigation between frames"
        );
        command(
            app.world_mut(),
            &handle,
            Action::Model,
            &ControlInput::Key(KeyChord::plain("ArrowLeft")),
        )
        .unwrap();
        settle(&mut app);
        assert_eq!(package.project_bytes(), archive);
        assert_eq!(app.world().resource::<Files>().script.source, source);
        assert_eq!(handle.presented_context(), Some(owner.clone()));
        assert_eq!(
            crate::native_viewport::interface_model_revision(app.world()),
            before_revision
        );
        assert_eq!(
            crate::native_viewport::interface_view_snapshot(app.world()),
            before_view
        );
        command(
            app.world_mut(),
            &handle,
            Action::Close,
            &ControlInput::Key(KeyChord::plain("Enter")),
        )
        .unwrap();
        released_textures(&mut app);
    }
}
