use nbcad_sketch::{EntityDto, LockedSegmentRequest, OriginPlane, PlaneRef, PointRequest, SetGridSnapRequest, SketchManager, Vec2};

#[test]
fn disabled_snapping_preserves_typed_endpoints_near_origin_and_other_points() {
    for nearby in [Vec2::new(0.,0.), Vec2::new(10.,10.)] {
        let mut manager=SketchManager::new();
        manager.begin_sketch(PlaneRef::OriginPlane{plane:OriginPlane::Xy}).unwrap();
        manager.add_point(PointRequest{position:nearby,ctrl_held:true,coincident_with:None}).unwrap();
        manager.set_grid_snap(SetGridSnapRequest{enabled:false}).unwrap();
        let target=nearby+Vec2::new(0.6,0.);
        let from=target-Vec2::new(0.,14.);
        let result=manager.add_line_locked(LockedSegmentRequest{from,to_hint:target,length_mm:Some(14.),angle_deg:Some(90.),length_text:None,angle_text:None,ctrl_held:true,tracking:None,intersection:None,from_crossing:None,to_crossing:None}).unwrap();
        let end=result.sketch.entities.iter().find_map(|e|match e{EntityDto::Line{end,..}=>Some(end),_=>None}).unwrap();
        assert!(end.distance(target)<1e-9,"Typed endpoint moved to a snap target: {end:?}");
        assert_eq!(result.sketch.constraints.len(),2,"Only the two requested driving dimensions should be added");
    }
}
