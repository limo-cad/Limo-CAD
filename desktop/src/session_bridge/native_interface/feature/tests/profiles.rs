use super::*;
use crate::native_viewport::{NativePickPurpose, ViewportCamera};
use crate::session_bridge::native_interface::controller::NativeServices;

#[test]
fn face_sketch_profiles_remain_selectable_at_bench_scale_without_picking_through_stock() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let mutate = |operation, arguments| {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, operation, &arguments, || Ok(()))
            .unwrap();
    };
    mutate("sketch_begin", json!({"type":"origin_plane","plane":"xy"}));
    mutate(
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":1200.,"y":85.},"ctrl_held":true}),
    );
    mutate("sketch_finish", json!({}));
    mutate(
        "solid_extrude",
        json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":35.}}),
    );
    let model = model_snapshot(&fixture.engine);
    let body = &model.document.scene.bodies[0];
    let top = body
        .faces
        .iter()
        .find(|face| face.plane.is_some_and(|plane| plane.normal[2] > 0.99))
        .unwrap()
        .id;
    let bottom = body
        .faces
        .iter()
        .find(|face| face.plane.is_some_and(|plane| plane.normal[2] < -0.99))
        .unwrap()
        .id;
    for face in [top, bottom] {
        mutate(
            "sketch_begin",
            json!({"plane":{"type":"planar_face","face_id":face},"face_origin":"global_origin_projection"}),
        );
        mutate(
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":63.,"y":28.},"p2":{"x":132.,"y":90.},"ctrl_held":true}),
        );
        mutate("sketch_finish", json!({}));
    }
    let mut app = scene(&fixture, &owner);
    native_viewport::apply_interface_viewport(
        app.world_mut(),
        limo_cad_interface::Rect {
            x: 0.,
            y: 0.,
            width: 1080.,
            height: 600.,
        },
        1.,
    )
    .unwrap();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let original = exported(&fixture);
    for camera_z in [5249.92333984375, 500., 50000.] {
        let camera = ViewportCamera {
            position: [600., 42.5, camera_z as f32],
            target: [600., 42.5, 17.5],
            up: [0., 1., 0.],
            ..Default::default()
        };
        for sketch_name in ["Sketch2", "Sketch3"] {
            native_viewport::apply_interface_view(
                app.world_mut(),
                &owner.document_id,
                Some(camera),
                Some(ViewportPresentation {
                    hidden_sketch_names: [
                        "Sketch1",
                        if sketch_name == "Sketch2" {
                            "Sketch3"
                        } else {
                            "Sketch2"
                        },
                    ]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                    ..Default::default()
                }),
            )
            .unwrap();
            let id = open(&fixture, app.world_mut(), &owner, None);
            action(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                FeatureControl::Pick(SolidField::Source),
                ControlInput::Click,
            )
            .unwrap();
            let cursor = native_viewport::interface_world_point(
                app.world(),
                &owner.document_id,
                [100., 50., 35.],
            )
            .unwrap()
            .unwrap();
            let hit = native_viewport::interface_pick(
                app.world(),
                &owner.document_id,
                cursor,
                NativePickPurpose::Geometry,
            )
            .unwrap()
            .unwrap();
            assert_eq!(hit.face_id, top.0);
            let basis = model_snapshot(&fixture.engine)
                .document
                .profile_catalog
                .iter()
                .find(|sketch| sketch.sketch_name == sketch_name)
                .unwrap()
                .basis;
            let local = native_viewport::interface_sketch_point(
                app.world(),
                &owner.document_id,
                cursor,
                basis,
            )
            .unwrap()
            .unwrap();
            let profile_distance = bevy::math::DVec3::from_array(camera.position.map(f64::from))
                .distance(bevy::math::DVec3::from_array(
                    basis.to_3d([local.x, local.y]),
                ));
            picking::handle_canvas_pick(app.world_mut(), &services, &owner, cursor)
                .unwrap()
                .unwrap();
            let selected = app
                .world()
                .resource::<NativeFeature>()
                .editor
                .as_ref()
                .unwrap()
                .form
                .selected_profiles();
            if sketch_name == "Sketch2" {
                assert_eq!(selected, vec![ProfileRefDto { sketch_name: sketch_name.into(), profile_index: 1 }], "camera_z={camera_z}, profile_depth={profile_distance}, mesh_depth={}, delta={}", hit.distance, profile_distance - hit.distance);
            } else {
                assert!(
                    selected.is_empty(),
                    "A rear-face sketch must not win through the stock"
                );
            }
            action(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                FeatureControl::Cancel,
                ControlInput::Click,
            )
            .unwrap();
            assert_eq!(exported(&fixture), original);
        }
    }
}

#[test]
fn initial_profile_hover_fills_only_the_closed_region_and_retires_on_cursor_leave() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "sketch_edit",
            &json!({"name":"Sketch1"}),
            || Ok(()),
        )
        .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "sketch_add_circle",
            &json!({"mode":"center_diameter","p1":{"x":10.,"y":6.},"p2":{"x":12.,"y":6.},"ctrl_held":true}),
            || Ok(()),
        )
        .unwrap();
    fixture
        .bridge
        .apply_native_mutation(&fixture.engine, &owner, "sketch_finish", &json!({}), || {
            Ok(())
        })
        .unwrap();
    let before = exported(&fixture);
    let mut app = scene(&fixture, &owner);
    native_viewport::apply_interface_viewport(
        app.world_mut(),
        limo_cad_interface::Rect {
            x: 0.,
            y: 0.,
            width: 1000.,
            height: 700.,
        },
        1.,
    )
    .unwrap();
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        Some(ViewportCamera {
            position: [10., 6., 100.],
            target: [10., 6., 0.],
            up: [0., 1., 0.],
            ..Default::default()
        }),
        Some(ViewportPresentation::default()),
    )
    .unwrap();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let id = open(&fixture, app.world_mut(), &owner, None);
    let project = |app: &bevy::app::App, p| {
        native_viewport::interface_world_point(app.world(), &owner.document_id, p)
            .unwrap()
            .unwrap()
    };
    let cursor = project(&app, [4., 4., 0.]);
    assert!(hover_references(app.world_mut(), &services, &owner, Some(cursor)).unwrap());
    assert_eq!(
        app.world()
            .resource::<NativeFeature>()
            .editor
            .as_ref()
            .unwrap()
            .hovered_profile
            .as_ref()
            .unwrap()
            .profile_index,
        0
    );
    assert!(!native_viewport::interface_preview_snapshot(app.world())
        .triangles
        .is_empty());
    assert!(
        !panel(app.world()).unwrap().can_apply,
        "Hover must not accept a profile"
    );
    let cursor = project(&app, [10., 6., 0.]);
    hover_references(app.world_mut(), &services, &owner, Some(cursor)).unwrap();
    assert!(
        app.world()
            .resource::<NativeFeature>()
            .editor
            .as_ref()
            .unwrap()
            .hovered_profile
            .is_none(),
        "A hole is not a selectable filled profile"
    );
    assert!(native_viewport::interface_preview_snapshot(app.world())
        .triangles
        .is_empty());
    let cursor = project(&app, [4., 4., 0.]);
    handle_canvas_pick(app.world_mut(), &services, &owner, cursor).unwrap();
    let accepted = native_viewport::interface_preview_snapshot(app.world());
    assert!(accepted.triangles.len() >= 2);
    hover_references(app.world_mut(), &services, &owner, None).unwrap();
    assert_eq!(
        native_viewport::interface_preview_snapshot(app.world()).triangles,
        accepted.triangles,
        "Cursor leave must keep the accepted extrusion volume"
    );
    assert_eq!(exported(&fixture), before);
    action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Cancel,
        ControlInput::Click,
    )
    .unwrap();
}
