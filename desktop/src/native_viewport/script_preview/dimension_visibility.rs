//! GPU regression for dimension arrows lying on a selected cube support face.
use super::*;
use crate::app_preferences::{palette::viewport_palette, ResolvedTheme};

#[test]
#[ignore = "requires a GPU; set LIMO_CAD_PREVIEW_PROOF_DIR to retain visual evidence"]
fn native_sketch_dimension_leaders_remain_visible_over_solid_faces() {
    let engine = crate::state::AppState::new();
    let call = |method: &str, args: serde_json::Value| {
        crate::session_bridge::parse_engine_envelope(engine.engine_call(method, &args.to_string()))
            .unwrap()
    };
    call(
        "begin_sketch",
        serde_json::json!({"type":"origin_plane","plane":"xy"}),
    );
    call(
        "add_rectangle",
        serde_json::json!({
            "mode":"two_point","p1":{"x":-5.,"y":-5.},"p2":{"x":5.,"y":5.},"ctrl_held":true
        }),
    );
    call("end_sketch", serde_json::json!({}));
    crate::session_bridge::parse_engine_envelope(
        engine.solid_extrude(
            &serde_json::json!({
                "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
                "extent":{"type":"distance","distance":10.},"taper_angle_deg":0.,
                "flip":false,"target_body_ids":[]
            })
            .to_string(),
        ),
    )
    .unwrap();
    let scene = engine.viewport_frame().document.scene.as_ref().clone();
    let top = scene.bodies[0]
        .faces
        .iter()
        .find(|face| face.plane.is_some_and(|plane| plane.normal[2] > 0.9))
        .unwrap()
        .id
        .0;
    let document = PreviewDocument::new(vec![Frame {
        assembly: None,
        caption: "Cube".into(),
        scene,
    }])
    .unwrap();
    let mut renderer = PreviewRenderer::new().unwrap();
    let cancelled = AtomicBool::new(false);
    let output = std::env::var_os("LIMO_CAD_PREVIEW_PROOF_DIR");
    if let Some(path) = &output {
        std::fs::create_dir_all(path).unwrap();
    }
    let mut request = RenderRequest {
        preview_id: "dimension-visibility".into(),
        view_id: String::new(),
        revision: 1,
        frame_index: 0,
        width: 800,
        height: 600,
        yaw: 0.,
        pitch: 1.3,
    };
    renderer
        .app
        .world_mut()
        .resource_mut::<PresentationResource>()
        .0 = ViewportPresentation {
        selected_face_ids: vec![top],
        hide_ground_grid: true,
        ..default()
    };
    for (theme, name) in [
        (ResolvedTheme::Light, "light"),
        (ResolvedTheme::Dark, "dark"),
    ] {
        apply_interface_palette(renderer.app.world_mut(), *viewport_palette(theme));
        for (pitch, view_name) in [(std::f32::consts::FRAC_PI_2, "top"), (0.8, "oblique")] {
            request.pitch = pitch;
            let camera = document.camera(&request);
            let viewport = ViewportSizeResource {
                logical_width: 800.,
                logical_height: 600.,
            };
            let arrow = world_per_pixel_at(camera, viewport, Vec3::new(0., 0., 10.)) * 6.;
            // Both 5 mm dimensions are entirely inside the cube projection.
            // Include extension lines, measurement lines and all arrow wings.
            let horizontal = [
                [Vec3::new(-2.5, -2.5, 10.), Vec3::new(-2.5, -4., 10.)],
                [Vec3::new(2.5, -2.5, 10.), Vec3::new(2.5, -4., 10.)],
                [Vec3::new(-2.5, -4., 10.), Vec3::new(2.5, -4., 10.)],
                [
                    Vec3::new(-2.5, -4., 10.),
                    Vec3::new(-2.5 + arrow, -4. + arrow * 0.4, 10.),
                ],
                [
                    Vec3::new(-2.5, -4., 10.),
                    Vec3::new(-2.5 + arrow, -4. - arrow * 0.4, 10.),
                ],
                [
                    Vec3::new(2.5, -4., 10.),
                    Vec3::new(2.5 - arrow, -4. + arrow * 0.4, 10.),
                ],
                [
                    Vec3::new(2.5, -4., 10.),
                    Vec3::new(2.5 - arrow, -4. - arrow * 0.4, 10.),
                ],
            ];
            let vertical = horizontal.map(|pair| pair.map(|p| Vec3::new(p.y, p.x, p.z)));
            let strokes: Vec<_> = horizontal.into_iter().chain(vertical).collect();
            let dimension = super::super::super::ViewportLineLayer {
                color: [1.; 4],
                color_role: super::super::super::ViewportColorRole::SketchDimension,
                width: 1.5,
                segments: strokes
                    .iter()
                    .flatten()
                    .flat_map(|p| p.to_array())
                    .collect::<Vec<_>>()
                    .into(),
                ..default()
            };
            let mut captures = Vec::new();
            for visible in [false, true] {
                renderer
                    .app
                    .world_mut()
                    .resource_mut::<PreviewResource>()
                    .sketch_lines = if visible {
                    vec![dimension.clone()]
                } else {
                    vec![]
                };
                let png = renderer
                    .render(
                        &document,
                        &request,
                        &cancelled,
                        Instant::now() + Duration::from_secs(60),
                    )
                    .unwrap();
                if let Some(path) = &output {
                    std::fs::write(
                        std::path::Path::new(path).join(format!(
                            "dimension-{name}-{view_name}-{}.png",
                            if visible { "with" } else { "without" }
                        )),
                        &png,
                    )
                    .unwrap();
                }
                captures.push(super::tests::decode_rgba(&png));
            }
            let (width, height, without) = &captures[0];
            let (_, _, with) = &captures[1];
            let view = camera_transform(camera).to_matrix().inverse();
            let projection = bevy::math::proj::perspective_infinite_reverse(
                camera.vertical_fov_degrees.to_radians(),
                *width as f32 / *height as f32,
                0.1,
            );
            for (index, [a, b]) in strokes.iter().enumerate() {
                let probes = (1..=9)
                    .map(|i| {
                        let clip = projection * view * a.lerp(*b, i as f32 / 10.).extend(1.);
                        let ndc = clip.truncate() / clip.w;
                        [
                            (ndc.x + 1.) * 0.5 * *width as f32,
                            (1. - ndc.y) * 0.5 * *height as f32,
                        ]
                    })
                    .collect::<Vec<_>>();
                let evidence =
                    boundary_evidence::visible_stroke(*width, *height, with, without, &probes)
                        .unwrap_or_else(|error| {
                            panic!("{name}/{view_name} leader {index}: {error}")
                        });
                assert_eq!(
                    evidence.visible_samples, evidence.samples,
                    "{name}/{view_name} leader {index} must remain visible along its length"
                );
            }
        }
    }
}
