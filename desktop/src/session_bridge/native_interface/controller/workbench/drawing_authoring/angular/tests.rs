use super::super::tests as fixtures;
use super::*;

fn stamp() -> Stamp {
    Stamp {
        owner: limo_cad_interface::DocumentContext {
            window_id: "main".into(),
            document_id: "drawing".into(),
            epoch: 7,
        },
        revision: 13,
        sheet_id: 1,
    }
}
fn anchors() -> [DrawingTopologyAnchorRefDto; 3] {
    let p = fixtures::projection();
    let a = super::super::anchors::endpoint_ref(&p.anchors[4], &p);
    let b = super::super::anchors::endpoint_ref(&p.anchors[5], &p);
    let mut c = b.clone();
    c.edge_key = "vertical".into();
    c.edge_id = limo_cad_core::EdgeId(7);
    c.fallback_point = [0., 30., 6.];
    [a, b, c]
}
#[test]
fn three_actual_anchors_preserve_exact_intent_and_reject_semantic_duplicates() {
    let [a, b, c] = anchors();
    let stamp = stamp();
    let mut p = Placement::default();
    assert!(p.click(&stamp, 1, a.clone(), [0., 0.]).unwrap().is_none());
    let mut duplicate = a.clone();
    duplicate.fallback_point = [42., 42., 42.];
    assert!(p.click(&stamp, 1, duplicate, [42., 42.]).unwrap().is_none());
    assert_eq!(p.picks.len(), 1);
    p.click(&stamp, 1, b.clone(), [40., 0.]).unwrap();
    let created = p.click(&stamp, 1, c.clone(), [0., 30.]).unwrap().unwrap();
    assert_eq!((created.vertex, created.first, created.second), (a, b, c));
    assert_eq!((created.radius, created.precision), (12., 1));
    assert!(p.picks.is_empty());
}
#[test]
fn geometry_rejects_zero_rays_and_zero_angle_but_preserves_a_valid_straight_angle() {
    assert!(valid_angle([0., 0.], [0., 0.], [1., 1.]).is_err());
    assert!(valid_angle([0., 0.], [1., 0.], [2., 0.]).is_err());
    assert!(valid_angle([0., 0.], [1., 0.], [-1., 0.]).is_ok());
    assert!(valid_angle([0., 0.], [1., 0.], [0., 1.]).is_ok());
    let [a, b, c] = anchors();
    let stamp = stamp();
    let mut p = Placement::default();
    p.click(&stamp, 1, a, [0., 0.]).unwrap();
    assert!(p.click(&stamp, 1, b.clone(), [0., 0.]).is_err());
    assert_eq!(p.picks.len(), 1);
    p.click(&stamp, 1, b, [40., 0.]).unwrap();
    assert!(p.click(&stamp, 1, c.clone(), [80., 0.]).is_err());
    assert_eq!(p.picks.len(), 2, "Reject only the invalid final pick");
    assert!(p.click(&stamp, 1, c, [0., 30.]).unwrap().is_some());
}
#[test]
fn view_revision_and_owner_changes_retire_angular_picks() {
    let [a, b, c] = anchors();
    let mut stamp = stamp();
    let mut p = Placement::default();
    p.click(&stamp, 1, a.clone(), [0., 0.]).unwrap();
    p.click(&stamp, 2, b.clone(), [40., 0.]).unwrap();
    assert_eq!(p.picks.len(), 1);
    assert!(p.selected(2, &b));
    assert!(!p.selected(1, &a));
    stamp.revision += 1;
    p.observe(&stamp);
    assert!(p.picks.is_empty());
    p.click(&stamp, 2, c, [0., 30.]).unwrap();
    stamp.owner.epoch += 1;
    p.observe(&stamp);
    assert!(p.picks.is_empty());
}
