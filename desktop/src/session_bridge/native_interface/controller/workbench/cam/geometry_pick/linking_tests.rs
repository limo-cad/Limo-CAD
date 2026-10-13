use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use operation_editor::linking_points::picking as linking;
use std::time::{Duration, Instant};

#[test]
fn linking_entry_exit_reuse_the_point_owner_and_commit_only_the_exact_draft_row() {
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for target in [linking::Target::Entry, linking::Target::Exit] {
        let fixture = Fixture::new();
        let (mut app, handle, services, receipt, _) = point_tests::setup(&fixture);
        let before_model =
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
        cancel(app.world_mut(), &handle);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<State>().worker.is_some() {
            tick(app.world_mut(), &handle, &services).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let cam = cam("contour2d");
        let mut draft = Draft::new(&cam, Selection::Operation(7)).unwrap();
        operation_editor::extend(&mut draft, &cam, &scene(), &[]).unwrap();
        form::set(&mut draft, "/native/ui/operation_section", "linking");
        form::set(&mut draft, "/native/linking/mode", "custom");
        let mut expected = draft.edited(&cam).unwrap();
        let target_path = linking::button(target.key());
        let editor = Editor {
            owner: Some(receipt.owner.clone()),
            revision: receipt.revision,
            cam,
            draft: Some(draft),
            tab: Tab::Toolpaths,
            ..default()
        };
        toggle_target(app.world_mut(), &handle, &receipt, &editor, &target_path).unwrap();
        app.world_mut().insert_resource(editor);
        assert!(target_matches(app.world(), &target_path));
        assert!(!target_matches(app.world(), setup::picking::BUTTON));
        while loading(app.world()) {
            tick(app.world_mut(), &handle, &services).unwrap();
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let bounds = viewport(&handle).unwrap();
        let p = native_viewport::interface_world_point(
            app.world(),
            &receipt.owner.document_id,
            [30., 0., -6.],
        )
        .unwrap()
        .unwrap();
        let cursor = Vec2::new(p[0] + bounds.x as f32, p[1] + bounds.y as f32);
        for press in [true, false] {
            let event = crate::native_viewport::winit_host::NativeHostInput {
                ui_scale: 1.,
                context: Some(receipt.owner.clone()),
                cursor: Some(cursor),
                modifiers: default(),
                consumed: false,
                actions: vec![],
                event: WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                    window: Entity::PLACEHOLDER,
                    button: MouseButton::Left,
                    state: if press {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                }),
            };
            assert!(input(app.world_mut(), &handle, &services, &event).unwrap());
        }
        assert!(!active(app.world()));
        assert!(overlay(app.world()).is_none());
        if target == linking::Target::Entry {
            expected.linking[0].entry_positions = vec![limo_cad_cam::Point2Dto::new(30., 0.)];
        } else {
            expected.linking[0].exit_positions = vec![limo_cad_cam::Point2Dto::new(30., 0.)];
        }
        let editor = app.world().resource::<Editor>();
        assert_eq!(
            editor.draft.as_ref().unwrap().edited(&editor.cam).unwrap(),
            expected
        );
        assert_eq!(
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
            before_model
        );
    }
}
