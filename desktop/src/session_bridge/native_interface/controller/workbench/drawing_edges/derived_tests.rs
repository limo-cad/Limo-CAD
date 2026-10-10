use super::*;
use serde_json::json;

fn sheet(derivation: serde_json::Value) -> DrawingSheetDto {
    let kind = derivation["type"].clone();
    serde_json::from_value(
        json!({"id":1,"name":"Derived pixels","format":"a4","orientation":"landscape",
        "views":[{"id":2,"name":"Derived","kind":kind,"position":[25.,25.],"scale":1.,
            "direction":[0.,0.,1.],"up":[0.,1.,0.],"derivation":derivation}]}),
    )
    .unwrap()
}
fn projection() -> DrawingProjectionDto {
    serde_json::from_value(json!({
        "topology_signatures":{"1":"current"}, "bounds":[0.,0.,30.,10.],
        "visible":[{"points":[[0.,5.],[30.,5.]]}],"hidden":[],"section":[],"circles":[],
        "anchors":[{"occurrence_id":null,"body_id":1,"edge_id":1,"edge_key":"center","endpoint":"start",
            "point":[15.,5.],"model_point":[15.,5.,0.],"hidden":false}]
    })).unwrap()
}
fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * image.texture_descriptor.size.width + x) * 4) as usize;
    image.data.as_ref().unwrap()[offset..offset + 4]
        .try_into()
        .unwrap()
}

#[test]
fn detail_clips_complete_paths_and_keeps_boundary_and_associations_across_pan_dpi() {
    let sheet = sheet(
        json!({"type":"detail","parent_view_id":1,"radius":6.,"label":"D",
        "center":{"body_id":1,"edge_id":1,"edge_key":"center","endpoint":"start",
            "topology_signature":"current","fallback_point":[999.,999.,999.]}}),
    );
    let key = SourceKey::new(tests::owner(), 1, 1, &sheet);
    let original = serde_json::to_value(&sheet).unwrap();
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let ready = cache
        .prepare(&mut images, key.clone(), tests::raster(), |_| {
            Ok(projection())
        })
        .unwrap();
    let handle = ready.image.clone();
    assert_eq!(
        ready.projections[&2].1.visible[0].points,
        vec![[0., 5.], [30., 5.]]
    );
    assert_eq!(ready.projections[&2].1.anchors.len(), 1);
    for dpi in [1., 2.] {
        let raster = RasterKey {
            render_scale: dpi,
            visible_mm: [20., 20., 20., 20.],
            ..tests::raster()
        };
        let ready = cache
            .prepare(&mut images, key.clone(), raster, |_| {
                panic!("Detail pan reprojected")
            })
            .unwrap();
        assert_eq!(ready.image, handle);
        let region = ready.region;
        let image = images.get(&handle).unwrap();
        let scale = f64::from(dpi);
        let at = |p: [f64; 2]| {
            [
                (p[0] - region.origin_mm[0]) * scale,
                (p[1] - region.origin_mm[1]) * scale,
            ]
            .map(|v| v.floor() as u32)
        };
        let [x, y] = at([25., 25.]);
        assert!(
            pixel(image, x, y)[3] > 0,
            "Visible line vanished inside detail circle"
        );
        let [x, y] = at([38., 25.]);
        assert_eq!(
            pixel(image, x, y)[3],
            0,
            "Complete underlying edge leaked outside circular clip"
        );
        let [x, y] = at([25., 19.]);
        assert!(
            pixel(image, x, y)[3] > 0,
            "Detail outline missing above source edges"
        );
        for y in 0..image.texture_descriptor.size.height {
            for x in 0..image.texture_descriptor.size.width {
                if pixel(image, x, y)[3] == 0 {
                    continue;
                }
                let point = [
                    region.origin_mm[0] + (x as f64 + 0.5) / scale,
                    region.origin_mm[1] + (y as f64 + 0.5) / scale,
                ];
                assert!((point[0] - 25.).hypot(point[1] - 25.) <= 6. + 1.5 / scale);
            }
        }
    }
    assert_eq!(serde_json::to_value(sheet).unwrap(), original);
}

#[test]
fn broken_white_mask_and_later_views_follow_saved_paint_order_instead_of_ids() {
    for axis in ["horizontal", "vertical"] {
        let mut sheet = sheet(
            json!({"type":"broken","parent_view_id":90,"axis":axis,"first":123.,"second":456.,"gap_mm":12.}),
        );
        let mut parent = sheet.views[0].clone();
        parent.id = 90;
        parent.derivation = None;
        sheet.views.insert(0, parent);
        let project = || {
            let mut p = projection();
            if axis == "vertical" {
                p.visible[0].points = vec![[15., -10.], [15., 20.]];
            }
            p
        };
        let original = serde_json::to_value(&sheet).unwrap();
        for reverse in [false, true] {
            if reverse {
                sheet.views.reverse();
            }
            let key = SourceKey::new(tests::owner(), 1, 1, &sheet);
            let source =
                Source::project(key, |_| Ok(project()), |_, _| Ok(vec![]), Limits::default())
                    .unwrap();
            let raster = tests::raster();
            let region = raster.region(&source.key, Limits::default()).unwrap();
            let image = source.rasterize(raster, region).unwrap();
            let color = pixel(&image, 25, 25);
            if reverse {
                assert!(
                    color[0] < 200,
                    "Later ordinary view was erased by earlier broken-view mask"
                );
            } else {
                assert_eq!(
                    color,
                    [255, 255, 255, 255],
                    "Mask did not clear earlier view through the centered paper-space gap"
                );
            }
        }
        sheet.views.reverse();
        assert_eq!(
            serde_json::to_value(&sheet).unwrap(),
            original,
            "Break placement changed saved first/second intent"
        );
    }
}

#[test]
fn aggregate_detail_mask_work_is_checked_and_irrelevant_clips_need_no_mask() {
    let mut sheet = sheet(
        json!({"type":"detail","parent_view_id":1,"radius":6.,"label":"D",
        "center":{"body_id":1,"edge_id":1,"edge_key":"center","endpoint":"start",
            "topology_signature":"current","fallback_point":[999.,999.,999.]}}),
    );
    let mut second = sheet.views[0].clone();
    second.id = 3;
    second.position = [75., 25.];
    sheet.views.push(second);
    let key = SourceKey::new(tests::owner(), 1, 1, &sheet);
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let ready = cache
        .prepare(&mut images, key.clone(), tests::raster(), |_| {
            Ok(projection())
        })
        .unwrap();
    let handle = ready.image.clone();
    let prior = images.get(&handle).unwrap().data.clone();
    let mut changed = key.clone();
    changed.document_revision += 1;
    let error = cache
        .prepare_with_limits(
            &mut images,
            changed.clone(),
            tests::raster(),
            |_| Ok(projection()),
            Limits {
                mask_pixels: 9000,
                ..Limits::default()
            },
        )
        .err()
        .unwrap();
    assert!(error.contains("raster work limit"), "{error}");
    assert_eq!(images.get(&handle).unwrap().data, prior);
    assert!(cache.projections(&changed).is_none());
    assert!(cache
        .prepare(&mut images, changed, tests::raster(), |_| panic!(
            "Failed masks retried projection"
        ))
        .is_err());

    let mut key = key;
    Arc::make_mut(&mut key.layout).views.truncate(1);
    for (center, radius) in [([200., 200.], 6.), ([25., 25.], 100.)] {
        Arc::make_mut(&mut key.layout).views[0].position = center;
        if let Some(DrawingViewDerivationDto::Detail { radius: r, .. }) =
            &mut Arc::make_mut(&mut key.layout).views[0].derivation
        {
            *r = radius;
        }
        let source = Source::project(
            key.clone(),
            |_| Ok(projection()),
            |_, _| Ok(vec![]),
            Limits::default(),
        )
        .unwrap();
        let raster = RasterKey {
            visible_mm: [20., 20., 10., 10.],
            ..tests::raster()
        };
        let region = raster.region(&key, Limits::default()).unwrap();
        let image = source
            .rasterize_with_limits(
                raster,
                region,
                Limits {
                    mask_pixels: 0,
                    ..Limits::default()
                },
            )
            .unwrap();
        let any = image
            .data
            .as_ref()
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] != 0);
        assert_eq!(any,radius==100.,"Disjoint detail must be empty; fully enclosing detail keeps visible edges without a mask");
    }
    if let Some(DrawingViewDerivationDto::Detail { center, .. }) =
        &mut Arc::make_mut(&mut key.layout).views[0].derivation
    {
        center.topology_signature = Some("retired".into());
    }
    let error = Source::project(
        key,
        |_| Ok(projection()),
        |_, _| Ok(vec![]),
        Limits::default(),
    )
    .err()
    .unwrap();
    assert!(error.contains("stale topology signature"), "{error}");
}

#[test]
fn real_detail_auxiliary_and_broken_sources_resolve_topology_without_mutating_the_model() {
    use crate::session_bridge::parse_engine_envelope;
    let (engine, mut sheet) = section_tests::fixture();
    let parent = sheet.views.pop().unwrap();
    sheet.views = vec![parent];
    let projection = engine
        .project_sheet_view(&sheet.views[0], &sheet.views)
        .unwrap();
    let start = projection
        .anchors
        .iter()
        .find(|a| {
            projection.anchors.iter().any(|b| {
                a.body_id == b.body_id
                    && a.edge_key == b.edge_key
                    && a.edge_id == b.edge_id
                    && a.endpoint != b.endpoint
                    && (a.point[0] - b.point[0]).hypot(a.point[1] - b.point[1]) > 10.
            })
        })
        .unwrap();
    let reference = json!({"body_id":start.body_id,"edge_id":start.edge_id,"edge_key":start.edge_key,
        "topology_signature":projection.topology_signatures[&start.body_id.0.to_string()],
        "fallback_start":[999.,999.,999.],"fallback_end":[888.,888.,888.]});
    let anchor = json!({"body_id":start.body_id,"edge_id":start.edge_id,"edge_key":start.edge_key,
        "endpoint":start.endpoint,"topology_signature":projection.topology_signatures[&start.body_id.0.to_string()],
        "fallback_point":[999.,999.,999.]});
    for (id, derivation) in [
        (
            2,
            json!({"type":"detail","parent_view_id":1,"center":anchor,"radius":12.,"label":"D"}),
        ),
        (
            3,
            json!({"type":"broken","parent_view_id":1,"axis":"horizontal","first":-18.,"second":16.,"gap_mm":6.}),
        ),
        (
            4,
            json!({"type":"broken","parent_view_id":1,"axis":"vertical","first":-11.,"second":13.,"gap_mm":5.}),
        ),
        (
            5,
            json!({"type":"auxiliary","parent_view_id":1,"reference":reference,"label":"A","flipped":false}),
        ),
        (
            6,
            json!({"type":"auxiliary","parent_view_id":1,"reference":reference,"label":"B","flipped":true}),
        ),
    ] {
        let mut child = sheet.views.last().unwrap().clone();
        child.id = id;
        child.kind = serde_json::from_value(derivation["type"].clone()).unwrap();
        child.position = [80. + id as f64 * 20., 80.];
        child.derivation = Some(serde_json::from_value(derivation).unwrap());
        sheet.views.insert(0, child);
    }
    let mut drawing = engine.drawing_snapshot();
    drawing.sheets[0] = sheet.clone();
    drawing.next_view_id = 7;
    drawing.validate().unwrap();
    parse_engine_envelope(engine.apply_encoded_mutate(
        "drawing_set_document",
        &serde_json::to_string(&drawing).unwrap(),
        false,
    ))
    .unwrap();
    let before = parse_engine_envelope(engine.engine_call("project_export_model", "")).unwrap();
    let key = SourceKey::new(tests::owner(), 1, engine.geometry_revision(), &sheet);
    let mut images = Assets::<Image>::default();
    let mut cache = EdgeCache::default();
    let ready = cache
        .prepare_sheet(
            &mut images,
            key.clone(),
            RasterKey {
                sheet_mm: [297., 210.],
                visible_mm: [0., 0., 297., 210.],
                ..tests::raster()
            },
            |view| engine.project_sheet_view_resolved(view, &sheet.views),
            |projections, budget| {
                engine.section_source_graphics(
                    &sheet,
                    |id| projections.get(&id).map(|(_, p)| p),
                    budget,
                )
            },
        )
        .unwrap();
    assert_eq!(ready.source_labels.len(), 3);
    assert!(ready
        .source_labels
        .iter()
        .all(|label| label.x.abs() < 500. && label.y.abs() < 500.));
    let source = cache.source.as_ref().unwrap();
    let captions = source
        .source_marks
        .iter()
        .filter_map(|primitive| {
            if let PaperPrimitive::Text {
                point,
                value,
                height,
                centered,
                rotation_deg,
                ..
            } = primitive
            {
                Some((point, value, height, centered, rotation_deg))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(captions.len(), source.source_labels.len());
    for (label, (point, value, height, centered, rotation_deg)) in
        source.source_labels.iter().zip(captions)
    {
        assert!(
            label.mask,
            "Derived caption {} needs the native white mask above its source strokes",
            label.text
        );
        assert_eq!(&label.text, value);
        assert_eq!(label.ink, super::super::Ink::Derived);
        let anchor_x = f64::from(label.x)
            - if *centered {
                0.
            } else {
                f64::from(label.width_mm) * 0.5
            };
        assert!((anchor_x - point[0]).abs() < 1e-5);
        assert_eq!(label.y, (point[1] - height * 0.4) as f32);
        assert_eq!(label.text_height_mm, *height as f32);
        assert_eq!(label.angle, rotation_deg.to_radians() as f32);
    }
    assert_eq!(
        source
            .source_marks
            .iter()
            .filter(|p| matches!(p, PaperPrimitive::Triangle { .. }))
            .count(),
        2
    );
    assert_eq!(source.projections.len(), 6);
    assert_eq!(
        source.projections[&2].1.bounds,
        source.projections[&1].1.bounds
    );
    assert_ne!(
        serde_json::to_value(&source.projections[&5].1).unwrap(),
        serde_json::to_value(&source.projections[&1].1).unwrap()
    );
    assert_eq!(
        parse_engine_envelope(engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
}
