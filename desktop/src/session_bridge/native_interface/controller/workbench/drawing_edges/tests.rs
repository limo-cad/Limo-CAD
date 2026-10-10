use super::*;
use limo_cad_occt::DrawingPolylineDto;
use serde_json::json;

fn sheet() -> DrawingSheetDto {
    serde_json::from_value(json!({"id":1,"name":"Dense sheet","format":"a4","orientation":"landscape",
        "views":[
            {"id":1,"name":"First","kind":"front","direction":[0.,-1.,0.],"up":[0.,0.,1.],"position":[25.,25.],"scale":1.,"show_hidden_lines":true},
            {"id":2,"name":"Later","kind":"front","direction":[0.,-1.,0.],"up":[0.,0.,1.],"position":[70.,50.],"scale":1.,"show_hidden_lines":true}
        ]})).unwrap()
}
pub(super) fn owner() -> DocumentContext {
    DocumentContext {
        window_id: "main".into(),
        document_id: "drawing-a".into(),
        epoch: 1,
    }
}
fn key() -> SourceKey {
    SourceKey::new(owner(), 19, 7, &sheet())
}
pub(super) fn raster() -> RasterKey {
    RasterKey {
        sheet_mm: [100., 80.],
        paper_scale: 1.,
        render_scale: 1.,
        visible_mm: [0., 0., 100., 80.],
    }
}
fn projection(dense: bool) -> DrawingProjectionDto {
    let mut points = if dense {
        (0..1024).map(|i| [i as f64 * 0.002, 0.]).collect()
    } else {
        vec![[0., 0.]]
    };
    points.push([30., 0.]);
    DrawingProjectionDto {
        topology_signatures: BTreeMap::from([("body".into(), "exact topology signature".into())]),
        visible: vec![DrawingPolylineDto { points }],
        hidden: vec![],
        anchors: vec![],
        circles: vec![],
        section: vec![],
        bounds: [0., 0., 30., 10.],
    }
}
fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * image.texture_descriptor.size.width + x) * 4) as usize;
    image.data.as_ref().unwrap()[offset..offset + 4]
        .try_into()
        .unwrap()
}

#[test]
fn idle_refresh_shares_source_layout_without_retagging_saved_projection_stamps() {
    let sheet = sheet();
    let owner = owner();
    let mut current = SourceKey::new(owner.clone(), 19, 7, &sheet);
    let saved = current.clone();
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let image = cache
        .prepare(&mut images, current.clone(), raster(), |_| {
            Ok(projection(false))
        })
        .unwrap()
        .image;
    for _ in 0..64 {
        current.refresh(&owner, 19, 7, &sheet);
        assert!(Arc::ptr_eq(&saved.layout, &current.layout));
        let ready = cache
            .prepare(&mut images, current.clone(), raster(), |_| {
                panic!("Idle drawing refresh must retain its projection")
            })
            .unwrap();
        assert_eq!(ready.image, image);
        assert!(!ready.source_changed);
    }

    current.refresh(&owner, 20, 7, &sheet);
    assert!(Arc::ptr_eq(&saved.layout, &current.layout));
    assert_eq!(saved.document_revision, 19);
    assert!(current != saved);
    cache.advance_sheet_selection(&owner, 19, 20);
    assert!(cache.projections(&saved).is_none());
    assert!(cache.projections(&current).is_some());
    assert!(Arc::ptr_eq(
        &saved.layout,
        &cache.source.as_ref().unwrap().key.layout
    ));
    let ready = cache
        .prepare(&mut images, current, raster(), |_| {
            panic!("Committed sheet selection must reuse its retained projection")
        })
        .unwrap();
    assert_eq!(ready.image, image);
    assert_eq!(images.len(), 1);
}

#[test]
fn section_and_removed_section_stroke_cut_edges_without_pruning_associations() {
    let anchor = json!({"body_id":1,"edge_id":1,"edge_key":"edge","endpoint":"start","fallback_point":[0.,0.,0.]});
    for (kind, ordinary, cut) in [
        ("section", true, true),
        ("removed_section", false, true),
        ("front", true, false),
    ] {
        let mut key = key();
        Arc::make_mut(&mut key.layout).views.truncate(1);
        if kind != "front" {
            Arc::make_mut(&mut key.layout).views[0].derivation = Some(
                serde_json::from_value(json!({
                    "type":kind,"parent_view_id":3,"first":anchor,"second":anchor,
                    "label":"A","hatch_angle_deg":45.,"hatch_spacing_mm":2.5
                }))
                .unwrap(),
            );
        }
        let mut data = projection(false);
        data.section.push(DrawingPolylineDto {
            points: vec![[0., 5.], [30., 5.], [30., 10.], [0., 10.], [0., 5.]],
        });
        let mut cache = EdgeCache::default();
        let mut images = Assets::<Image>::default();
        let ready = cache
            .prepare(&mut images, key, raster(), |_| Ok(data.clone()))
            .unwrap();
        let image = images.get(&ready.image).unwrap();
        assert_eq!(
            pixel(image, 25, 30)[3] > 0,
            ordinary,
            "ordinary layer {kind}"
        );
        assert_eq!(pixel(image, 25, 25)[3] > 0, cut, "section layer {kind}");
        assert_eq!(
            serde_json::to_value(&ready.projections[&1].1).unwrap(),
            serde_json::to_value(&data).unwrap(),
            "Visibility must not prune associations"
        );
    }
}

#[test]
fn dense_projection_retains_every_segment_and_all_later_view_associations() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let ready = cache
        .prepare(&mut images, key(), raster(), |view| {
            Ok(projection(view.id == 1))
        })
        .unwrap();
    assert_eq!(ready.projections.len(), 2);
    assert_eq!(ready.projections[&1].1.visible[0].points.len(), 1025);
    assert_eq!(
        ready.projections[&2].1.topology_signatures["body"],
        "exact topology signature"
    );
    let image = images.get(&ready.image).unwrap();
    assert!(
        pixel(image, 38, 30)[3] > 0,
        "Final segment beyond the old 800 edge cap is missing"
    );
    assert!(
        pixel(image, 80, 55)[3] > 0,
        "A later view was silently skipped"
    );
}

#[test]
fn returning_to_a_sheet_moves_its_pixels_without_reprojection_or_rasterization() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let handle = cache
        .prepare(&mut images, key(), raster(), |_| Ok(projection(false)))
        .unwrap()
        .image;
    let original = images.get(&handle).unwrap().data.clone();
    let original_pixels = images.get(&handle).unwrap().data.as_ref().unwrap().as_ptr();
    cache.advance_sheet_selection(&owner(), 19, 20);
    let mut second = key();
    Arc::make_mut(&mut second.layout).sheet_id = 2;
    second.document_revision = 20;
    Arc::make_mut(&mut second.layout).views[0].position[1] += 20.;
    cache
        .prepare(&mut images, second, raster(), |_| Ok(projection(true)))
        .unwrap();
    assert_ne!(images.get(&handle).unwrap().data, original);
    cache.advance_sheet_selection(&owner(), 20, 21);
    let mut returned = key();
    returned.document_revision = 21;
    assert!(cache
        .prepare(
            &mut images,
            returned.clone(),
            RasterKey {
                paper_scale: 1000.,
                ..raster()
            },
            |_| panic!("Oversized crop projected")
        )
        .is_err());
    let ready = cache
        .prepare(&mut images, returned, raster(), |_| {
            panic!("Warm source projected")
        })
        .unwrap();
    assert!(ready.source_changed);
    assert_eq!(ready.image, handle);
    let restored = images.get(&ready.image).unwrap();
    assert_eq!(restored.data, original);
    assert_eq!(
        restored.data.as_ref().unwrap().as_ptr(),
        original_pixels,
        "The existing pixel buffer must be moved, without redrawing or copying it"
    );
    assert_eq!(images.len(), 1);
}

#[test]
fn warm_pixels_share_the_pixel_budget_and_changed_dpi_rasterizes_current_geometry() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let handle = cache
        .prepare(&mut images, key(), raster(), |_| Ok(projection(false)))
        .unwrap()
        .image;
    let original = images.get(&handle).unwrap().data.clone();
    let pixels = raster_bytes(images.get(&handle).unwrap()) / 4;
    cache.advance_sheet_selection(&owner(), 19, 20);
    let mut second = key();
    Arc::make_mut(&mut second.layout).sheet_id = 2;
    second.document_revision = 20;
    cache
        .prepare_with_limits(
            &mut images,
            second,
            raster(),
            |_| Ok(projection(true)),
            Limits {
                pixels: pixels * 2 - 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(
        cache.previous_source.is_some(),
        "Geometry may remain warm without pixels"
    );
    assert!(
        cache.previous_raster.is_none(),
        "Do not raise the retained pixel budget"
    );
    cache.advance_sheet_selection(&owner(), 20, 21);
    let mut returned = key();
    returned.document_revision = 21;
    cache
        .prepare(&mut images, returned, raster(), |_| {
            panic!("Evicting pixels lost geometry")
        })
        .unwrap();
    assert_eq!(images.get(&handle).unwrap().data, original);
    let mut scaled = raster();
    scaled.render_scale = 2.;
    cache
        .prepare(
            &mut images,
            {
                let mut next = key();
                next.document_revision = 21;
                next
            },
            scaled,
            |_| panic!("DPI change projected geometry"),
        )
        .unwrap();
    let image = images.get(&handle).unwrap();
    assert_eq!(image.texture_descriptor.size.width, 200);
    assert_eq!(images.len(), 1);
    let previous_bytes = cache
        .previous_raster
        .as_ref()
        .map_or(0, |(_, _, image)| raster_bytes(image));
    assert!(raster_bytes(image) + previous_bytes <= Limits::default().pixels * 4);
}

#[test]
fn sheet_selection_reuses_the_previous_sheet_but_edits_and_undo_reproject() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let first = cache
        .prepare(&mut images, key(), raster(), |_| Ok(projection(false)))
        .unwrap()
        .image;
    cache.advance_sheet_selection(&owner(), 19, 20);
    let mut second = key();
    Arc::make_mut(&mut second.layout).sheet_id = 2;
    second.document_revision = 20;
    cache
        .prepare(&mut images, second, raster(), |_| Ok(projection(true)))
        .unwrap();
    cache.advance_sheet_selection(&owner(), 20, 21);
    let mut returned = key();
    returned.document_revision = 21;
    let warm = cache
        .prepare(&mut images, returned.clone(), raster(), |_| {
            panic!("Returning to a selected sheet must reuse its complete projection")
        })
        .unwrap();
    assert!(
        warm.source_changed,
        "Paper annotations must be rebound on a warm switch"
    );
    assert_eq!(warm.image, first);
    assert_eq!(warm.projections[&1].1.visible[0].points.len(), 2);
    assert_eq!(images.len(), 1);
    for revision in [22, 23] {
        returned.document_revision = revision;
        let mut calls = 0;
        cache
            .prepare(&mut images, returned.clone(), raster(), |_| {
                calls += 1;
                Ok(projection(false))
            })
            .unwrap();
        assert_eq!(calls, 2);
    }
}

#[test]
fn warm_sheet_retag_requires_the_exact_owner_and_an_adjacent_revision() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    cache
        .prepare(&mut images, key(), raster(), |_| Ok(projection(false)))
        .unwrap();
    let mut other = owner();
    other.epoch += 1;
    cache.advance_sheet_selection(&other, 19, 20);
    cache.advance_sheet_selection(&owner(), 18, 19);
    cache.advance_sheet_selection(&owner(), 19, 21);
    assert_eq!(cache.source.as_ref().unwrap().key.document_revision, 19);
    cache.advance_sheet_selection(&owner(), 19, 20);
    let mut changed = key();
    changed.document_revision = 20;
    changed.geometry_revision += 1;
    let mut calls = 0;
    cache
        .prepare(&mut images, changed, raster(), |_| {
            calls += 1;
            Ok(projection(false))
        })
        .unwrap();
    assert_eq!(
        calls, 2,
        "Retagging cannot authorize changed solid geometry"
    );
}

#[test]
fn warm_sources_share_the_retained_geometry_budget_and_failed_switches_are_atomic() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    cache
        .prepare(&mut images, key(), raster(), |_| Ok(projection(false)))
        .unwrap();
    let mut second = key();
    Arc::make_mut(&mut second.layout).sheet_id = 2;
    cache
        .prepare(&mut images, second.clone(), raster(), |_| {
            Ok(projection(false))
        })
        .unwrap();
    let original = images
        .get(&cache.raster.as_ref().unwrap().2)
        .unwrap()
        .data
        .clone();
    assert!(cache
        .prepare(
            &mut images,
            key(),
            RasterKey {
                paper_scale: 1000.,
                ..raster()
            },
            |_| { panic!("An oversized warm raster must fail before projection") }
        )
        .is_err());
    assert_eq!(
        images.get(&cache.raster.as_ref().unwrap().2).unwrap().data,
        original
    );
    assert_eq!(cache.source.as_ref().unwrap().key.layout.sheet_id, 2);
    assert_eq!(
        cache.previous_source.as_ref().unwrap().key.layout.sheet_id,
        1
    );
    cache
        .prepare(&mut images, key(), raster(), |_| {
            panic!("Failure evicted the warm sheet")
        })
        .unwrap();
    let limits = Limits {
        retained_bytes: cache.source.as_ref().unwrap().retained_bytes * 2 - 1,
        ..Default::default()
    };
    let mut third = key();
    Arc::make_mut(&mut third.layout).sheet_id = 3;
    cache
        .prepare_with_limits(
            &mut images,
            third,
            raster(),
            |_| Ok(projection(false)),
            limits,
        )
        .unwrap();
    assert!(cache.previous_source.is_none());
    let mut calls = 0;
    cache
        .prepare(&mut images, second, raster(), |_| {
            calls += 1;
            Ok(projection(false))
        })
        .unwrap();
    assert_eq!(calls, 2);
}

#[test]
fn edge_cache_reuses_exact_source_and_repaints_at_changed_dpi_size_style_or_owner() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let first = cache
        .prepare(&mut images, key(), raster(), |_| Ok(projection(false)))
        .unwrap()
        .image;
    let unchanged = cache
        .prepare(&mut images, key(), raster(), |_| {
            panic!("Unchanged view reprojected")
        })
        .unwrap();
    assert!(!unchanged.source_changed);
    assert_eq!(unchanged.image, first);
    let mut twice = raster();
    twice.render_scale = 2.;
    let scaled = cache
        .prepare(&mut images, key(), twice, |_| {
            panic!("DPI must not reproject OCCT")
        })
        .unwrap();
    assert!(!scaled.source_changed);
    assert_eq!(scaled.image, first);
    assert_eq!(
        images
            .get(&scaled.image)
            .unwrap()
            .texture_descriptor
            .size
            .width,
        200
    );
    assert!(pixel(images.get(&scaled.image).unwrap(), 160, 110)[3] > 0);
    for x in [10., 20.] {
        let zoom = RasterKey {
            paper_scale: 3. * 4.23,
            render_scale: 2.,
            visible_mm: [x, 10., 60., 40.],
            ..raster()
        };
        let ready = cache
            .prepare(&mut images, key(), zoom, |_| {
                panic!("Zoom/pan reprojected OCCT")
            })
            .unwrap();
        assert!(!ready.source_changed);
        assert_eq!(ready.image, first);
        assert_eq!(
            serde_json::to_value(&ready.projections[&1].1).unwrap(),
            serde_json::to_value(projection(false)).unwrap()
        );
        assert_eq!(images.len(), 1);
    }
    for variant in 0..15 {
        let mut next = key();
        let mut owner = owner();
        let mut sheet = sheet();
        let mut document_revision = 19;
        let mut geometry_revision = 7;
        match variant {
            0 => document_revision += 1,
            1 => geometry_revision += 1,
            2 => owner.epoch += 1,
            3 => owner.document_id = "drawing-b".into(),
            4 => sheet.views[0].position[0] += 1.,
            5 => sheet.style.visible.width_mm *= 2.,
            6 => sheet.style.hidden.width_mm *= 2.,
            7 => sheet.style.hatch.width_mm *= 2.,
            8 => sheet.style.cutting_plane.width_mm *= 2.,
            9 => sheet.style.phantom.width_mm *= 2.,
            10 => sheet.style.break_line.width_mm *= 2.,
            11 => sheet.style.hatch_spacing_mm *= 2.,
            12 => sheet.style.text_height_mm *= 2.,
            13 => sheet.id += 1,
            _ => owner.window_id = "other-window".into(),
        }
        next.refresh(&owner, document_revision, geometry_revision, &sheet);
        assert!(
            next != key(),
            "Changed source variant {variant} reused its key"
        );
        let mut calls = 0;
        assert!(
            cache
                .prepare(&mut images, next, raster(), |_| {
                    calls += 1;
                    Ok(projection(false))
                })
                .unwrap()
                .source_changed
        );
        assert_eq!(calls, 2);
        assert_eq!(
            images.len(),
            1,
            "Resizing/editing must reuse the retained image allocation"
        );
    }
}

#[test]
fn failed_or_oversized_render_is_explicit_atomic_and_not_retried_without_a_change() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let first = cache
        .prepare(&mut images, key(), raster(), |_| Ok(projection(false)))
        .unwrap()
        .image;
    let original = images.get(&first).unwrap().data.clone();
    let mut changed = key();
    Arc::make_mut(&mut changed.layout).owner.epoch += 1;
    let mut calls = 0;
    let error = cache
        .prepare_with_limits(
            &mut images,
            changed.clone(),
            raster(),
            |_| {
                calls += 1;
                Ok(projection(true))
            },
            Limits {
                points: 100,
                ..Default::default()
            },
        )
        .err()
        .unwrap();
    assert!(error.contains("retained geometry budget"));
    assert_eq!(calls, 1);
    assert!(cache
        .prepare(&mut images, changed, raster(), |_| panic!(
            "Same failed source retried"
        ))
        .is_err());
    assert_eq!(images.get(&first).unwrap().data, original);
    assert_eq!(cache.source.as_ref().unwrap().key.layout.owner, owner());
    let huge = RasterKey {
        paper_scale: 1000.,
        ..raster()
    };
    assert!(cache
        .prepare(&mut images, key(), huge, |_| panic!(
            "Oversized image must reject before OCCT"
        ))
        .err()
        .unwrap()
        .contains("physical pixels"));
    assert_eq!(images.get(&first).unwrap().data, original);
    let bad = cache
        .prepare(
            &mut images,
            {
                let mut k = key();
                k.geometry_revision += 1;
                k
            },
            raster(),
            |_| Err("HLR failed".into()),
        )
        .err()
        .unwrap();
    assert!(bad.contains("First") && bad.contains("HLR failed"));
}

#[test]
fn hidden_dash_phase_runs_across_tessellated_segments_and_odd_patterns_repeat() {
    let mut k = key();
    Arc::make_mut(&mut k.layout).views.truncate(1);
    Arc::make_mut(&mut k.layout).views[0].position = [50., 30.];
    for dash in [vec![4., 2.], vec![2.]] {
        Arc::make_mut(&mut k.layout).hidden.dash_mm = dash.clone();
        let source = Source::project(
            k.clone(),
            |_| {
                Ok(DrawingProjectionDto {
                    visible: vec![],
                    hidden: vec![DrawingPolylineDto {
                        points: (0..=100).map(|x| [x as f64, 0.]).collect(),
                    }],
                    bounds: [0., 0., 100., 10.],
                    ..projection(false)
                })
            },
            |_, _| Ok(vec![]),
            Limits::default(),
        )
        .unwrap();
        let region = raster().region(&source.key, Limits::default()).unwrap();
        let image = source.rasterize(raster(), region).unwrap();
        assert!(pixel(&image, 1, 35)[3] > 0);
        let gap = if dash.len() == 2 { 5 } else { 3 };
        assert_eq!(
            pixel(&image, gap, 35)[3],
            0,
            "Dash phase restarted at every tessellation point"
        );
    }
}

#[test]
fn subpixel_widths_remain_visible_at_fractional_positions_and_both_dpi_scales() {
    let mut k = key();
    Arc::make_mut(&mut k.layout).views.truncate(1);
    Arc::make_mut(&mut k.layout).views[0].position = [25.25, 25.25];
    Arc::make_mut(&mut k.layout).visible.width_mm = 0.05;
    let source = Source::project(
        k,
        |_| Ok(projection(false)),
        |_, _| Ok(vec![]),
        Limits::default(),
    )
    .unwrap();
    for dpi in [1., 2.] {
        let key = RasterKey {
            render_scale: dpi,
            ..raster()
        };
        let region = key.region(&source.key, Limits::default()).unwrap();
        let image = source.rasterize(key, region).unwrap();
        let x = (38. * dpi) as u32;
        assert!(((29. * dpi) as u32..=(32. * dpi) as u32).any(|y| pixel(&image, x, y)[3] > 0));
    }
}

#[test]
fn maximum_zoom_rasterizes_only_the_visible_region_without_losing_late_edges_or_projections() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let raster = RasterKey {
        sheet_mm: [1000., 800.],
        paper_scale: 15.,
        render_scale: 2.,
        visible_mm: [35., 28., 12., 4.],
    };
    let first = cache
        .prepare(&mut images, key(), raster, |v| Ok(projection(v.id == 1)))
        .unwrap();
    assert_eq!(first.projections.len(), 2);
    assert_eq!(first.projections[&1].1.visible[0].points.len(), 1025);
    let handle = first.image.clone();
    let region = first.region;
    let image = images.get(&handle).unwrap();
    assert_eq!(image.texture_descriptor.size.width, 360 + 64);
    assert_eq!(image.texture_descriptor.size.height, 120 + 64);
    let factor = 30.;
    let point = [38., 30.];
    let local = [
        ((point[0] - region.origin_mm[0]) * factor) as u32,
        ((point[1] - region.origin_mm[1]) * factor) as u32,
    ];
    assert!(
        pixel(image, local[0], local[1])[3] > 0,
        "Last dense-polyline segment was lost after ROI clipping"
    );
    let pan = RasterKey {
        visible_mm: [75., 53., 12., 4.],
        ..raster
    };
    let second = cache
        .prepare(&mut images, key(), pan, |_| {
            panic!("Pan must not reproject the solid")
        })
        .unwrap();
    assert!(!second.source_changed);
    assert_eq!(second.image, handle);
    assert_eq!(
        second.projections[&2].1.topology_signatures["body"],
        "exact topology signature"
    );
    let local = [
        ((80. - second.region.origin_mm[0]) * factor) as u32,
        ((55. - second.region.origin_mm[1]) * factor) as u32,
    ];
    assert!(pixel(images.get(&second.image).unwrap(), local[0], local[1])[3] > 0);
    assert_eq!(images.len(), 1);
}

#[test]
fn region_clips_to_paper_and_uses_exact_scale_instead_of_stretching_fractional_pixel_dimensions() {
    let mut k = key();
    Arc::make_mut(&mut k.layout).views.truncate(1);
    let raster = RasterKey {
        paper_scale: 1.37,
        render_scale: 2.,
        visible_mm: [-100., -20., 1000., 1000.],
        ..raster()
    };
    let region = raster.region(&k, Limits::default()).unwrap();
    assert_eq!(region.origin_mm, [0.; 2]);
    let factor = f64::from(raster.paper_scale) * f64::from(raster.render_scale);
    for i in 0..2 {
        assert!((region.size_mm[i] * factor - f64::from(region.dimensions[i])).abs() < 1e-9);
        assert!(region.size_mm[i] >= f64::from(raster.sheet_mm[i]));
        assert!(region.size_mm[i] - f64::from(raster.sheet_mm[i]) < factor.recip());
    }
    for bad in [
        [1000., 0., 10., 10.],
        [0., 0., 0., 10.],
        [f64::NAN, 0., 10., 10.],
    ] {
        assert!(RasterKey {
            visible_mm: bad,
            ..raster
        }
        .region(&k, Limits::default())
        .is_err());
    }
}

#[test]
fn panning_a_crop_preserves_hidden_dash_phase_from_the_complete_polyline() {
    let mut k = key();
    Arc::make_mut(&mut k.layout).views.truncate(1);
    Arc::make_mut(&mut k.layout).views[0].position = [50., 30.];
    let source = Source::project(
        k,
        |_| {
            Ok(DrawingProjectionDto {
                visible: vec![],
                hidden: vec![DrawingPolylineDto {
                    points: (0..=100).map(|x| [x as f64, 0.]).collect(),
                }],
                bounds: [0., 0., 100., 10.],
                ..projection(false)
            })
        },
        |_, _| Ok(vec![]),
        Limits::default(),
    )
    .unwrap();
    let full_key = raster();
    let full = source
        .rasterize(
            full_key,
            full_key.region(&source.key, Limits::default()).unwrap(),
        )
        .unwrap();
    let cropped_key = RasterKey {
        visible_mm: [9., 34., 12., 2.],
        ..full_key
    };
    let region = cropped_key.region(&source.key, Limits::default()).unwrap();
    let cropped = source.rasterize(cropped_key, region).unwrap();
    for x in 9..21 {
        for y in 34..36 {
            assert_eq!(
                pixel(&full, x, y),
                pixel(
                    &cropped,
                    x - region.origin_mm[0] as u32,
                    y - region.origin_mm[1] as u32
                ),
                "Cropping changed the retained polyline dash at ({x}, {y})"
            );
        }
    }
}

#[test]
fn resolved_projection_basis_is_cached_by_exact_owner_revision_without_rewriting_view_intent() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let key = key();
    let saved = serde_json::to_value(&key.layout.views).unwrap();
    let mut projected = 0;
    let basis = limo_cad_occt::drawing_projection_basis([0., 0., 1.], [0., 1., 0.]).unwrap();
    cache
        .prepare_sheet(
            &mut images,
            key.clone(),
            raster(),
            |_| {
                projected += 1;
                Ok(ResolvedDrawingProjection {
                    projection: projection(false),
                    basis,
                })
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
    assert_eq!(projected, key.layout.views.len());
    assert_eq!(cache.bases(&key).unwrap().len(), key.layout.views.len());
    for (id, (view, _)) in cache.projections(&key).unwrap() {
        assert_eq!(cache.bases(&key).unwrap()[id], basis);
        assert_eq!(view.direction, [0., -1., 0.]);
    }
    assert_eq!(serde_json::to_value(&key.layout.views).unwrap(), saved);
    cache
        .prepare_sheet(
            &mut images,
            key.clone(),
            RasterKey {
                render_scale: 2.,
                ..raster()
            },
            |_| panic!("DPI re-ran projection"),
            |_, _| panic!("DPI rebuilt source graphics"),
        )
        .unwrap();
    for variant in 0..3 {
        let mut changed = key.clone();
        match variant {
            0 => changed.document_revision += 1,
            1 => Arc::make_mut(&mut changed.layout).owner.epoch += 1,
            _ => Arc::make_mut(&mut changed.layout).owner.document_id = "other".into(),
        };
        assert!(cache.projections(&changed).is_none());
        assert!(cache.bases(&changed).is_none());
        assert!(cache
            .prepare_sheet(
                &mut images,
                changed.clone(),
                raster(),
                |_| Err("new owner projection failed".into()),
                |_, _| Ok(vec![])
            )
            .is_err());
        assert!(
            cache.bases(&changed).is_none(),
            "Failed request exposed previous owner basis"
        );
    }
    let mut fresh = key.clone();
    fresh.document_revision += 4;
    let flipped = limo_cad_occt::drawing_projection_basis([0., 0., -1.], [0., 1., 0.]).unwrap();
    cache
        .prepare_sheet(
            &mut images,
            fresh.clone(),
            raster(),
            |_| {
                Ok(ResolvedDrawingProjection {
                    projection: projection(false),
                    basis: flipped,
                })
            },
            |_, _| Ok(vec![]),
        )
        .unwrap();
    assert!(cache.bases(&key).is_none());
    assert_eq!(cache.bases(&fresh).unwrap()[&1], flipped);
}

#[test]
fn native_retention_purges_inactive_paper_caches_and_keeps_active_raster() {
    let mut cache = EdgeCache::default();
    let mut images = Assets::<Image>::default();
    let cold = key();
    cache
        .prepare(&mut images, cold.clone(), raster(), |_| {
            Ok(projection(false))
        })
        .unwrap();
    let mut active = cold.clone();
    Arc::make_mut(&mut active.layout).owner.document_id = "drawing-b".into();
    let ready = cache
        .prepare(&mut images, active.clone(), raster(), |_| {
            Ok(projection(true))
        })
        .unwrap();
    let active_image = ready.image.clone();
    assert!(cache.previous_source.is_some());
    assert!(cache.previous_raster.is_some());
    cache.evict_document(&cold.layout.owner);
    assert!(cache.previous_source.is_none());
    assert!(cache.previous_raster.is_none());
    assert!(cache.projections(&active).is_some());
    assert_eq!(cache.raster.as_ref().unwrap().2, active_image);
    let ready = cache
        .prepare(&mut images, active, raster(), |_| {
            panic!("Active projections must remain warm")
        })
        .unwrap();
    assert!(!ready.source_changed);
    assert_eq!(ready.image, active_image);
    cache.evict_document(&cache.source.as_ref().unwrap().key.layout.owner.clone());
    assert!(cache.source.is_none());
    assert!(cache.raster.is_none());
}
