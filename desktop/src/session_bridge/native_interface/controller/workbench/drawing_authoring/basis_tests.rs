//! Real OCCT coincident targets must rank along the resolved derived basis,
//! while every saved DrawingViewDto remains untouched.
use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use limo_cad_occt::{DrawingProjectionAnchorEndpoint, DrawingProjectionDto};
use limo_cad_sketch::{DrawingRadialDimensionMode, DrawingViewDto};
use serde_json::json;
fn depth(p: [f64; 3], direction: [f64; 3]) -> f64 {
    (0..3).map(|i| p[i] * direction[i]).sum()
}
fn same_point(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7
}
fn fixture() -> Fixture {
    let f = Fixture::new();
    for (name, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":6.}}),
        ),
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_circle",
            json!({"mode":"center_diameter","p1":{"x":60.,"y":15.},"p2":{"x":63.,"y":15.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch2","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.}}),
        ),
    ] {
        f.bridge
            .apply_native_mutation(&f.engine, &f.owner(), name, &args, || Ok(()))
            .unwrap();
    }
    f
}
fn assert_front_endpoints(
    view: &DrawingViewDto,
    p: &DrawingProjectionDto,
    direction: [f64; 3],
) -> usize {
    let actual = anchors::endpoints(view, p, direction).unwrap();
    let old = anchors::endpoints(view, p, view.direction).unwrap();
    let mut changed = 0;
    for chosen in actual {
        let expected = p
            .anchors
            .iter()
            .filter(|a| !a.hidden && same_point(a.point, chosen.point))
            .map(|a| depth(a.model_point, direction))
            .reduce(f64::max)
            .unwrap();
        assert!(
            (depth(chosen.model_point, direction) - expected).abs() < 1e-7,
            "Selected rear endpoint {chosen:?} along resolved direction {direction:?}"
        );
        let old = old
            .iter()
            .find(|a| same_point(a.point, chosen.point))
            .unwrap();
        if depth(old.model_point, direction) < expected - 1e-7 {
            changed += 1;
        }
    }
    changed
}
#[test]
fn detail_and_nested_broken_pick_actual_front_circles_and_endpoints_without_rewriting_views() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = fixture();
    let top = tests::document().sheets[0].views[0].clone();
    let first = f
        .engine
        .project_sheet_view_resolved(&top, std::slice::from_ref(&top))
        .unwrap();
    let top_centers = center::targets(&top, &first.projection, first.basis.direction).unwrap();
    assert_eq!(top_centers.len(), 1);
    assert_eq!(top_centers[0].reference.fallback_center[2], 10.);
    let mut scaled = top.clone();
    scaled.direction = [0., 0., 1e-6];
    scaled.up = [0., 7., 3.];
    let scaled_result = f
        .engine
        .project_sheet_view_resolved(&scaled, &[scaled.clone()])
        .unwrap();
    assert_eq!(scaled_result.basis, first.basis);
    assert_eq!(
        serde_json::to_value(&scaled_result.projection.anchors).unwrap(),
        serde_json::to_value(&first.projection.anchors).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&scaled_result.projection.circles).unwrap(),
        serde_json::to_value(&first.projection.circles).unwrap()
    );
    let circle = first
        .projection
        .circles
        .iter()
        .find(|c| !c.hidden && c.center_model[2] > 9.)
        .unwrap();
    let center = json!({"body_id":circle.body_id,"edge_id":circle.edge_id,"edge_key":circle.edge_key,"endpoint":"start","circle_center":true,"topology_signature":first.projection.topology_signatures[&circle.body_id.0.to_string()].clone(),"fallback_point":circle.center_model});
    let detail:DrawingViewDto=serde_json::from_value(json!({"id":2,"name":"Detail opposite stored direction","kind":"detail","direction":[0.,0.,-1.],"up":[0.,1.,0.],"position":[140.,100.],"scale":1.,"show_hidden_lines":false,"derivation":{"type":"detail","parent_view_id":1,"center":center,"radius":100.,"label":"D"}})).unwrap();
    let broken:DrawingViewDto=serde_json::from_value(json!({"id":3,"name":"Nested broken opposite direction","kind":"broken","direction":[0.,0.,-1.],"up":[0.,1.,0.],"position":[180.,100.],"scale":1.,"show_hidden_lines":false,"derivation":{"type":"broken","parent_view_id":2,"axis":"horizontal","first":140.,"second":150.,"gap_mm":8.}})).unwrap();
    let views = vec![top, detail, broken];
    let saved = serde_json::to_value(&views).unwrap();
    let model = || parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap();
    let before = model();
    for view in &views[1..] {
        let resolved = f.engine.project_sheet_view_resolved(view, &views).unwrap();
        assert_eq!(resolved.basis.direction, [0., 0., 1.]);
        assert!(
            assert_front_endpoints(view, &resolved.projection, resolved.basis.direction) > 0,
            "Real derived projection must reproduce the old reversed-depth endpoint bug"
        );
        let circles = radial::targets(
            view,
            &resolved.projection,
            resolved.basis.direction,
            DrawingRadialDimensionMode::Radius,
        )
        .unwrap();
        let old = radial::targets(
            view,
            &resolved.projection,
            view.direction,
            DrawingRadialDimensionMode::Radius,
        )
        .unwrap();
        assert!(
            circles.len() >= 2,
            "Actual OCCT has coincident visible top/base circle edges"
        );
        let point = [
            circles[0].center[0] + circles[0].radius,
            circles[0].center[1],
        ];
        let picked = &circles[radial::hit(&circles, point, 1e-6).unwrap()];
        let wrong = &old[radial::hit(&old, point, 1e-6).unwrap()];
        assert_eq!(picked.reference.fallback_center[2], 10.);
        assert_eq!(wrong.reference.fallback_center[2], 0.);
        let centers =
            center::targets(view, &resolved.projection, resolved.basis.direction).unwrap();
        assert_eq!(centers.len(), 1);
        assert_eq!(centers[0].reference, picked.reference);
        let old_centers = center::targets(view, &resolved.projection, view.direction).unwrap();
        assert_eq!(old_centers[0].reference.fallback_center[2], 0.);
        let linear =
            anchors::circles(view, &resolved.projection, resolved.basis.direction, false).unwrap();
        assert_eq!(linear.len(), 1);
        assert_eq!(linear[0].edge_id, picked.reference.edge_id);
        assert_eq!(linear[0].center_model, picked.reference.fallback_center);
        assert_eq!(
            picked.reference.topology_signature,
            Some(
                resolved.projection.topology_signatures[&picked.reference.body_id.0.to_string()]
                    .clone()
            )
        );
    }
    assert_eq!(serde_json::to_value(&views).unwrap(), saved);
    assert_eq!(model(), before);
}
#[test]
fn flipped_auxiliary_uses_its_resolved_basis_for_real_coincident_targets() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = fixture();
    let top = tests::document().sheets[0].views[0].clone();
    let p = f
        .engine
        .project_sheet_view(&top, std::slice::from_ref(&top))
        .unwrap();
    let (a, b) = p
        .anchors
        .iter()
        .filter(|a| a.endpoint == DrawingProjectionAnchorEndpoint::Start)
        .find_map(|a| {
            p.anchors
                .iter()
                .find(|b| {
                    b.body_id == a.body_id
                        && b.edge_id == a.edge_id
                        && b.endpoint == DrawingProjectionAnchorEndpoint::End
                        && (a.model_point[0] - b.model_point[0]).abs() > 39.
                        && (a.model_point[1] - b.model_point[1]).abs() < 1e-7
                        && (a.model_point[2] - b.model_point[2]).abs() < 1e-7
                })
                .map(|b| (a, b))
        })
        .unwrap();
    let reference = json!({"body_id":a.body_id,"edge_id":a.edge_id,"edge_key":a.edge_key,"topology_signature":p.topology_signatures[&a.body_id.0.to_string()],"fallback_start":a.model_point,"fallback_end":b.model_point});
    let mut directions = Vec::new();
    let mut changed = 0;
    for flipped in [false, true] {
        let child:DrawingViewDto=serde_json::from_value(json!({"id":2,"name":"Auxiliary","kind":"auxiliary","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[150.,100.],"scale":1.,"show_hidden_lines":false,"derivation":{"type":"auxiliary","parent_view_id":1,"reference":reference,"flipped":flipped,"label":"A"}})).unwrap();
        let saved = serde_json::to_value(&child).unwrap();
        let views = vec![top.clone(), child.clone()];
        let resolved = f
            .engine
            .project_sheet_view_resolved(&child, &views)
            .unwrap();
        assert!(resolved.basis.direction[2].abs() < 1e-7);
        assert!((resolved.basis.direction[1].abs() - 1.).abs() < 1e-7);
        changed += assert_front_endpoints(&child, &resolved.projection, resolved.basis.direction);
        directions.push(resolved.basis.direction);
        assert_eq!(serde_json::to_value(&child).unwrap(), saved);
    }
    assert!(
        changed > 0,
        "Actual auxiliary projection must exercise old stored-basis ties"
    );
    for (first, second) in directions[0].iter().zip(&directions[1]) {
        assert!((first + second).abs() < 1e-7);
    }
}

#[test]
fn normalized_projection_depth_tolerance_is_independent_of_stored_vector_length() {
    let mut view = tests::document().sheets[0].views[0].clone();
    view.direction = [0., 0., 1e-6];
    view.up = [0., 7., 3.];
    let saved = serde_json::to_value(&view).unwrap();
    let basis = limo_cad_occt::drawing_projection_basis(view.direction, view.up).unwrap();
    assert_eq!(
        basis,
        limo_cad_occt::drawing_projection_basis([0., 0., 1.], [0., 1., 0.]).unwrap()
    );
    let mut p = tests::projection();
    p.anchors.truncate(2);
    p.anchors[0].edge_id = limo_cad_core::EdgeId(1);
    p.anchors[0].model_point[2] = 0.;
    p.anchors[1].edge_id = limo_cad_core::EdgeId(2);
    p.anchors[1].model_point[2] = 0.01;
    assert_eq!(anchors::endpoints(&view,&p,view.direction).unwrap()[0].edge_id.0,1,"Reproduce raw 1e-6 direction collapsing a 0.01mm front/back span into the depth tie tolerance");
    assert_eq!(
        anchors::endpoints(&view, &p, basis.direction).unwrap()[0]
            .edge_id
            .0,
        2
    );
    assert_eq!(serde_json::to_value(&view).unwrap(), saved);
}
