use super::*;
use crate::{
    session_bridge::{dispatch_inbox_on_engine, parse_engine_envelope},
    state::AppState,
};
use limo_cad_occt::drawing_export::PaperPrimitive;
use serde_json::{json, Value};

pub(super) fn fixture() -> (AppState, DrawingSheetDto) {
    let engine = AppState::new();
    let mutate = |name: &str, args: Value| {
        dispatch_inbox_on_engine(&engine, name, &args).unwrap_or_else(|e| panic!("{name}: {e}"));
    };
    for (name, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":-20.,"y":-15.},"p2":{"x":20.,"y":15.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":20.}}),
        ),
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_circle",
            json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":6.,"y":0.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch2","profile_indices":[0],"operation":"cut","target_body_ids":[1],"extent":{"type":"distance","distance":20.}}),
        ),
        (
            "drawing_create_sheet",
            json!({"name":"Section validation","format":"a4","orientation":"landscape"}),
        ),
        (
            "drawing_add_view",
            json!({"sheet_id":1,"view":{"name":"Top","kind":"top","body_ids":[1],"direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[65.,65.],"scale":1.}}),
        ),
    ] {
        mutate(name, args);
    }
    let mut sheet = engine.drawing_snapshot().sheets.remove(0);
    let top = &sheet.views[0];
    let projection = engine.project_sheet_view(top, &sheet.views).unwrap();
    let circle = projection
        .circles
        .iter()
        .find(|circle| !circle.hidden && circle.closed)
        .unwrap();
    let corner = projection
        .anchors
        .iter()
        .find(|a| {
            (a.model_point[0] - 20.).abs() < 1e-6
                && (a.model_point[1] - 15.).abs() < 1e-6
                && (a.model_point[2] - circle.center_model[2]).abs() < 1e-6
        })
        .unwrap();
    let signature = &projection.topology_signatures[&circle.body_id.0.to_string()];
    let first = json!({"body_id":circle.body_id,"edge_id":circle.edge_id,"edge_key":circle.edge_key,
        "endpoint":"start","circle_center":true,"topology_signature":signature,"fallback_point":[900000.,900000.,900000.]});
    let second = json!({"body_id":corner.body_id,"edge_id":corner.edge_id,"edge_key":corner.edge_key,
        "endpoint":corner.endpoint,"topology_signature":signature,"fallback_point":[800000.,800000.,800000.]});
    let section: DrawingViewDto = serde_json::from_value(json!({
        "id":2,"name":"Section A-A","kind":"section","body_ids":[1],"direction":[0.6,-0.8,0.],"up":[0.,0.,1.],
        "position":[165.,65.],"scale":1.,"show_hidden_lines":true,
        "derivation":{"type":"section","parent_view_id":1,"first":first,"second":second,
            "label":"Section A-A","hatch_angle_deg":17.,"hatch_spacing_mm":2.}
    })).unwrap();
    let mut removed = serde_json::to_value(&section).unwrap();
    removed["id"] = json!(3);
    removed["name"] = json!("Removed B-B");
    removed["kind"] = json!("removed_section");
    removed["position"] = json!([165., 125.]);
    removed["derivation"]["type"] = json!("removed_section");
    removed["derivation"]["label"] = json!("B-B");
    sheet.views.insert(0, section);
    sheet
        .views
        .insert(0, serde_json::from_value(removed).unwrap());
    sheet.style.hatch_spacing_mm = 5.;
    let mut drawing = engine.drawing_snapshot();
    drawing.sheets[0] = sheet;
    drawing.next_view_id = 4;
    parse_engine_envelope(engine.apply_encoded_mutate(
        "drawing_set_document",
        &serde_json::to_string(&drawing).unwrap(),
        false,
    ))
    .unwrap();
    let saved = engine.drawing_snapshot().sheets.remove(0);
    (engine, saved)
}

fn model(engine: &AppState) -> Value {
    parse_engine_envelope(engine.engine_call("project_export_model", "")).unwrap()
}

#[test]
fn real_section_projection_hatching_source_marks_and_cached_navigation_preserve_intent() {
    let (engine, sheet) = fixture();
    let before = model(&engine);
    let section = &sheet.views[1];
    let cut = engine.project_sheet_view(section, &sheet.views).unwrap();
    assert!(
        !cut.section.is_empty(),
        "Native section must run the exact cutting plane"
    );
    let mut ordinary = section.clone();
    ordinary.derivation = None;
    assert!(engine
        .project_sheet_view(&ordinary, &sheet.views)
        .unwrap()
        .section
        .is_empty());
    let mut limited = section.clone();
    if let Some(DrawingViewDerivationDto::Section { depth, .. }) = &mut limited.derivation {
        *depth = Some(4.);
    }
    let depth = engine.project_sheet_view(&limited, &sheet.views).unwrap();
    assert!(!depth.section.is_empty());
    assert_ne!(
        serde_json::to_value(&depth.visible).unwrap(),
        serde_json::to_value(&cut.visible).unwrap(),
        "Finite depth must clip actual HLR"
    );

    let key = SourceKey::new(tests::owner(), 1, engine.geometry_revision(), &sheet);
    let raster = RasterKey {
        sheet_mm: [297., 210.],
        visible_mm: [0., 0., 297., 210.],
        ..tests::raster()
    };
    let mut images = Assets::<Image>::default();
    let mut cache = EdgeCache::default();
    let ready = cache
        .prepare_sheet(
            &mut images,
            key.clone(),
            raster,
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
    assert_eq!(ready.source_labels.len(), 4);
    assert!(ready
        .source_labels
        .iter()
        .all(|l| matches!(l.ink, super::super::Ink::Derived)));
    let image = ready.image.clone();
    let source = cache.source.as_ref().unwrap();
    assert!(!source.hatches.is_empty());
    assert_eq!(
        source
            .source_marks
            .iter()
            .filter(|p| matches!(p, PaperPrimitive::Triangle { .. }))
            .count(),
        4
    );
    let projection = &source.projections[&2].1;
    let center = paper_point(section, [0., 10.], projection);
    for primitive in &source.hatches {
        if let PaperPrimitive::Line { points, .. } = primitive {
            let mid = [
                (points[0][0] + points[1][0]) * 0.5,
                (points[0][1] + points[1][1]) * 0.5,
            ];
            if (mid[1] - center[1]).abs() < 3. {
                assert!(
                    (mid[0] - center[0]).abs() > 2.,
                    "Hatch entered the physical through-hole"
                );
            }
        }
    }
    for dpi in [1., 2.] {
        let moved = RasterKey {
            render_scale: dpi,
            visible_mm: [140., 40., 55., 55.],
            ..raster
        };
        let ready = cache
            .prepare_sheet(
                &mut images,
                key.clone(),
                moved,
                |_| panic!("Pan/DPI re-ran OCCT"),
                |_, _| panic!("Pan/DPI rebuilt source marks"),
            )
            .unwrap();
        assert!(!ready.source_changed);
        assert_eq!(ready.image, image);
        assert_eq!(ready.source_labels.len(), 4);
        let pixels = images.get(&image).unwrap().data.as_ref().unwrap();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 0 && p[0] > 70 && p[0] < 150),
            "Section hatch pixels missing"
        );
    }
    assert_eq!(model(&engine), before);

    let projected_points = cache
        .source
        .as_ref()
        .unwrap()
        .projections
        .values()
        .map(|(_, p)| {
            p.visible
                .iter()
                .chain(&p.hidden)
                .chain(&p.section)
                .map(|line| line.points.len())
                .sum::<usize>()
                + p.anchors.len()
                + p.circles.len()
        })
        .sum();
    let exhausted = SourceKey::new(tests::owner(), 3, engine.geometry_revision(), &sheet);
    let previous_image = images.get(&image).unwrap().data.clone();
    let error = cache
        .prepare_sheet_with_limits(
            &mut images,
            exhausted.clone(),
            raster,
            |view| engine.project_sheet_view_resolved(view, &sheet.views),
            |projections, budget| {
                engine.section_source_graphics(
                    &sheet,
                    |id| projections.get(&id).map(|(_, p)| p),
                    budget,
                )
            },
            Limits {
                points: projected_points,
                ..Limits::default()
            },
        )
        .err()
        .unwrap();
    assert!(error.contains("point limit"), "{error}");
    assert!(cache.projections(&exhausted).is_none());
    assert_eq!(images.get(&image).unwrap().data, previous_image);
    assert_eq!(model(&engine), before);

    let mut stale = sheet.clone();
    if let Some(DrawingViewDerivationDto::Section { first, .. }) = &mut stale.views[1].derivation {
        first.edge_key = "replaced-topology".into();
    }
    let changed = SourceKey::new(tests::owner(), 2, engine.geometry_revision(), &stale);
    let error = cache
        .prepare_sheet(
            &mut images,
            changed.clone(),
            raster,
            |view| engine.project_sheet_view_resolved(view, &stale.views),
            |projections, budget| {
                engine.section_source_graphics(
                    &stale,
                    |id| projections.get(&id).map(|(_, p)| p),
                    budget,
                )
            },
        )
        .err()
        .unwrap();
    assert!(!error.is_empty());
    assert!(
        cache.projections(&changed).is_none(),
        "A stale derived view cannot expose prior cached geometry"
    );
    assert_eq!(model(&engine), before);

    let mut oversized = sheet.clone();
    oversized.style.text_height_mm = 1e100;
    let mut accepted = engine.drawing_snapshot();
    accepted.sheets[0] = oversized.clone();
    accepted.validate().unwrap();
    let huge = SourceKey::new(tests::owner(), 4, engine.geometry_revision(), &oversized);
    let error = cache
        .prepare_sheet(
            &mut images,
            huge.clone(),
            raster,
            |view| engine.project_sheet_view_resolved(view, &oversized.views),
            |projections, budget| {
                engine.section_source_graphics(
                    &oversized,
                    |id| projections.get(&id).map(|(_, p)| p),
                    budget,
                )
            },
        )
        .err()
        .unwrap();
    assert!(error.contains("section label"), "{error}");
    assert!(cache.projections(&huge).is_none());
    assert_eq!(images.get(&image).unwrap().data, previous_image);
    assert_eq!(model(&engine), before);
}
