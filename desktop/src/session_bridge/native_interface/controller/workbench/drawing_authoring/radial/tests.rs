use super::super::tests as fixtures;
use super::*;

#[test]
fn concentric_circular_edges_stay_individually_pickable_with_exact_signatures() {
    let p = fixtures::projection();
    let d = fixtures::document();
    let view = &d.sheets[0].views[0];
    let circles = targets(view, &p, view.direction, DrawingRadialDimensionMode::Radius).unwrap();
    assert_eq!(circles.len(), 2);
    for (index, c) in circles.iter().enumerate() {
        assert_eq!(
            hit(&circles, [c.center[0] + c.radius, c.center[1]], 0.5),
            Some(index)
        );
        assert_eq!(
            c.reference.topology_signature.as_deref(),
            Some("exact-body-topology")
        );
        assert_eq!(c.reference.fallback_normal, [0., 0., 1.]);
        assert_eq!(c.reference.edge_key, p.circles[index].edge_key);
        assert_eq!(c.reference.fallback_radius, p.circles[index].radius);
    }
    assert_eq!(
        hit(&circles, circles[0].center, 0.5),
        None,
        "Circle centers are not circle-edge hits"
    );
    assert_eq!(hit(&circles, [f64::NAN, 1.], 0.5), None);
}
#[test]
fn hidden_and_open_arc_eligibility_matches_the_existing_tools() {
    let mut p = fixtures::projection();
    let mut d = fixtures::document();
    let view = &mut d.sheets[0].views[0];
    view.show_hidden_lines = false;
    p.circles[0].hidden = true;
    p.circles[1].closed = false;
    assert!(targets(
        view,
        &p,
        view.direction,
        DrawingRadialDimensionMode::Diameter
    )
    .unwrap()
    .is_empty());
    let arcs = targets(view, &p, view.direction, DrawingRadialDimensionMode::Radius).unwrap();
    assert_eq!(arcs.len(), 1);
    assert!(!arcs[0].reference.closed);
    let stamp = Stamp {
        owner: limo_cad_interface::DocumentContext {
            window_id: "main".into(),
            document_id: "drawing".into(),
            epoch: 1,
        },
        revision: 1,
        sheet_id: 1,
    };
    assert!(request(&stamp, &arcs[0], DrawingRadialDimensionMode::Diameter).is_err());
    let radius = request(&stamp, &arcs[0], DrawingRadialDimensionMode::Radius).unwrap();
    assert_eq!(
        (radius.leader_angle_deg, radius.offset, radius.precision),
        (-35., 14., 2)
    );
    view.show_hidden_lines = true;
    assert_eq!(
        targets(
            view,
            &p,
            view.direction,
            DrawingRadialDimensionMode::Diameter
        )
        .unwrap()
        .len(),
        1
    );
    assert_eq!(
        targets(view, &p, view.direction, DrawingRadialDimensionMode::Radius)
            .unwrap()
            .len(),
        2
    );
}
